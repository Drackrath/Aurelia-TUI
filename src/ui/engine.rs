//! The runtime-plugins overlay (modal popup): the three compatibility plugins —
//! umu-launcher, luxtorpeda, and the Steam Linux Runtime — with their status and
//! the highlighted row for enable/disable/install/update/uninstall (or
//! install/repair for the runtime).

use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::interface::aurelia;
use crate::theme;
use crate::ui::{paneled_paragraph, selection_marker};

/// One-line status for a umu/luxtorpeda plugin.
fn plugin_desc(status: Option<&aurelia::PluginStatusJson>) -> String {
    match status {
        None => "status unavailable".to_string(),
        Some(s) => {
            let enabled = if s.enabled { "enabled" } else { "disabled" };
            match &s.installed {
                Some(inst) => format!("{enabled} · installed {}", inst.version),
                None => format!("{enabled} · not installed"),
            }
        }
    }
}

/// One-line status for the Steam Linux Runtime.
fn steam_runtime_desc(status: Option<&aurelia::SteamRuntimeStatusJson>) -> String {
    match status {
        None => "status unavailable".to_string(),
        Some(s) => {
            let present = if s.steam_exe_present { "present" } else { "missing" };
            format!("{} · steam {}", s.layout_kind, present)
        }
    }
}

/// Build the runtime-plugins overlay from the browser's open engine state. Three
/// rows (umu, luxtorpeda, Steam Linux Runtime) show status; the highlighted row
/// gets a selection bar. The hint line adapts to the selected plugin.
pub fn engine(browser: &Browser) -> Paragraph<'static> {
    let rows: [(&str, String); 3] = [
        ("umu-launcher", plugin_desc(browser.engine_umu.as_ref())),
        ("luxtorpeda", plugin_desc(browser.engine_lux.as_ref())),
        (
            "Steam Linux Runtime",
            steam_runtime_desc(browser.engine_steam_runtime.as_ref()),
        ),
    ];

    let mut lines: Vec<Spans<'static>> = Vec::new();
    for (i, (name, desc)) in rows.iter().enumerate() {
        let selected = i == browser.engine_index;
        let style = if selected {
            theme::selection(theme::ACCENT)
        } else {
            theme::value()
        };
        lines.push(Spans::from(vec![
            Span::styled(selection_marker(selected).to_string(), style),
            Span::styled(format!("{name}  "), style),
            Span::styled(desc.clone(), theme::dim()),
        ]));
    }

    lines.push(Spans::from(""));
    let hint = if browser.engine_index == 2 {
        "[i] install  [r] repair  [Esc] close"
    } else {
        "[e] enable  [d] disable  [i] install  [U] update  [x] uninstall  [Esc] close"
    };
    let status = if let Some(busy) = browser.engine_busy_text() {
        busy
    } else if !browser.engine_status.is_empty() {
        browser.engine_status.clone()
    } else {
        hint.to_string()
    };
    lines.push(Spans::from(Span::styled(status, theme::dim())));

    paneled_paragraph(lines, "Runtime plugins".to_string())
}
