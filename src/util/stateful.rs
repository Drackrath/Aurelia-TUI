use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use tui::widgets::ListState;

pub trait Named {
    fn get_name(&self) -> String;
    fn is_valid(&self) -> bool;
}

#[derive(Clone)]
pub struct StatefulList<T> {
    pub state: ListState,
    pub items: Vec<T>,
    pub query: String,
}

impl<T: Named> StatefulList<T> {
    pub fn new() -> StatefulList<T> {
        StatefulList {
            state: ListState::default(),
            items: Vec::new(),
            query: "".to_string(),
        }
    }

    pub fn with_items(items: Vec<T>) -> StatefulList<T> {
        StatefulList {
            state: ListState::default(),
            items,
            query: "".to_string(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn selected(&self) -> Option<&T> {
        // `activated()` is re-filtered by the live `query`, so the stored index
        // can outrun the visible list (e.g. the query grew after a selection was
        // made). Treat an out-of-range index as "no selection" rather than
        // panicking — `next`/`previous` keep it in range in the common path, and
        // this stays safe if a future caller drives selection differently.
        let i = self.state.selected()?;
        self.activated().get(i).copied()
    }

    pub fn activated(&self) -> Vec<&T> {
        let matcher = SkimMatcherV2::default();
        self.items
            .iter()
            .filter(|nameable| {
                matcher
                    .fuzzy_match(&nameable.get_name(), &self.query)
                    .is_some()
            })
            .filter(|nameable| nameable.is_valid())
            .collect::<Vec<_>>()
    }

    pub fn restart(&mut self) {
        if self.activated().is_empty() {
            self.state.select(None);
        } else {
            self.state.select(Some(0));
        }
    }

    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.activated().len() - 1 {
                    Some(0)
                } else {
                    Some(i + 1)
                }
            }
            None => {
                if !self.activated().is_empty() {
                    Some(0)
                } else {
                    None
                }
            }
        };
        self.state.select(i);
    }

    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    Some(self.activated().len() - 1)
                } else {
                    Some(i - 1)
                }
            }
            None => {
                if !self.activated().is_empty() {
                    Some(0)
                } else {
                    None
                }
            }
        };
        self.state.select(i);
    }
    pub fn unselect(&mut self) {
        self.state.select(None);
    }
}

impl<T: Named> Default for StatefulList<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Item(String);
    impl Named for Item {
        fn get_name(&self) -> String {
            self.0.clone()
        }
        fn is_valid(&self) -> bool {
            true
        }
    }

    #[test]
    fn selected_out_of_range_does_not_panic() {
        // Select the last row, then narrow the live query so `activated()`
        // shrinks below the stored index. `selected()` must yield `None`
        // instead of panicking (the old `.expect()` would have aborted here).
        let mut list = StatefulList::with_items(vec![
            Item("alpha".to_string()),
            Item("beta".to_string()),
        ]);
        list.state.select(Some(1));
        list.query = "alpha".to_string(); // now only one item is activated
        assert!(list.selected().is_none());
    }

    #[test]
    fn selected_in_range_returns_item() {
        let mut list = StatefulList::with_items(vec![Item("alpha".to_string())]);
        list.state.select(Some(0));
        assert_eq!(list.selected().map(|i| i.0.clone()), Some("alpha".to_string()));
    }
}
