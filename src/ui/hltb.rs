//! Compact completion-time rows for the detail pane.
use crate::interface::hltb::HltbState;

/// Round to whole minutes without turning a real sub-minute time into "0m".
/// HLTB uses zero for an unsubmitted style, which shares the missing-data marker.
fn duration(seconds: Option<u64>) -> String {
    let Some(seconds) = seconds.filter(|s| *s > 0) else {
        return "—".into();
    };
    let minutes = (seconds.saturating_add(30) / 60).max(1);
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// Pack complete play-style/time groups before wrapping individual words. The
/// detail pane reserves one table row of this height, so keeping the groups
/// compact leaves room for existing installation and description content.
pub(super) fn lines(state: &HltbState, width: usize) -> Option<Vec<String>> {
    let text: &str = match state {
        HltbState::Ready { data, stale } => {
            let mut parts = vec![
                format!("Main Story {}", duration(data.main)),
                format!("Main + Extras {}", duration(data.extra)),
                format!("Completionist {}", duration(data.completionist)),
            ];
            // Expired estimates stay visible during refresh and after failure;
            // neither state implies that the user is offline.
            if *stale {
                parts.insert(0, "Cached".into());
            }
            let mut lines = Vec::new();
            let mut line = String::new();
            for part in parts {
                let candidate = if line.is_empty() {
                    part.clone()
                } else {
                    format!("{line} · {part}")
                };
                if candidate.chars().count() <= width.max(1) {
                    line = candidate;
                } else {
                    if !line.is_empty() {
                        lines.push(std::mem::take(&mut line));
                    }
                    // Only split a group if it cannot fit on an otherwise empty
                    // line; ordinary wrapping could separate "10h" from "32m".
                    let mut wrapped = super::detail::wrap(&part, width.max(1));
                    line = wrapped.pop().unwrap_or_default();
                    lines.extend(wrapped);
                }
            }
            if !line.is_empty() {
                lines.push(line);
            }
            return Some(lines);
        }
        HltbState::Pending | HltbState::Loading => return None,
        HltbState::NotFound => "No verified match",
        HltbState::Unavailable(_) => "Fetch Failed",
    };
    Some(super::detail::wrap(text, width.max(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_short_times_are_distinct() {
        assert_eq!(duration(None), "—");
        assert_eq!(duration(Some(0)), "—");
        assert_eq!(duration(Some(1)), "1m");
        assert_eq!(duration(Some(3600)), "1h");
        assert_eq!(duration(Some(11230)), "3h 7m");
    }
}
