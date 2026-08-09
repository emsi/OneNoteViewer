use onenote_core::{PageId, SectionId, SourceId};

const MAX_HISTORY_ENTRIES: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PageLocation {
    pub(crate) source: SourceId,
    pub(crate) section: SectionId,
    pub(crate) page: PageId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryDirection {
    Back,
    Forward,
}

#[derive(Debug)]
pub(crate) struct NavigationHistory {
    entries: Vec<PageLocation>,
    current: Option<usize>,
    capacity: usize,
}

impl Default for NavigationHistory {
    fn default() -> Self {
        Self::with_capacity(MAX_HISTORY_ENTRIES)
    }
}

impl NavigationHistory {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            current: None,
            capacity: capacity.max(1),
        }
    }

    pub(crate) fn current(&self) -> Option<&PageLocation> {
        self.current.and_then(|position| self.entries.get(position))
    }

    pub(crate) fn can_go_back(&self) -> bool {
        self.current.is_some_and(|position| position > 0)
    }

    pub(crate) fn can_go_forward(&self) -> bool {
        self.current
            .is_some_and(|position| position + 1 < self.entries.len())
    }

    pub(crate) fn record(&mut self, location: PageLocation) {
        if self.current() == Some(&location) {
            return;
        }
        if let Some(position) = self.current {
            self.entries.truncate(position + 1);
        } else {
            self.entries.clear();
        }
        self.entries.push(location);
        if self.entries.len() > self.capacity {
            let excess = self.entries.len() - self.capacity;
            self.entries.drain(..excess);
        }
        self.current = Some(self.entries.len() - 1);
    }

    pub(crate) fn replace_current(&mut self, location: PageLocation) {
        let Some(position) = self.current else {
            self.record(location);
            return;
        };
        if self.entries[position] == location {
            return;
        }
        self.entries[position] = location;
        self.coalesce_current();
    }

    pub(crate) fn step(&mut self, direction: HistoryDirection) -> Option<PageLocation> {
        let current = self.current?;
        let next = match direction {
            HistoryDirection::Back => current.checked_sub(1)?,
            HistoryDirection::Forward => {
                (current + 1 < self.entries.len()).then_some(current + 1)?
            }
        };
        self.current = Some(next);
        self.entries.get(next).cloned()
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&PageLocation) -> bool) {
        let previous_current = self.current;
        let mut retained = Vec::with_capacity(self.entries.len());
        let mut retained_at_or_before_current = None;
        for (position, entry) in self.entries.drain(..).enumerate() {
            if keep(&entry) {
                if retained.last() == Some(&entry) {
                    if previous_current.is_some_and(|current| position <= current) {
                        retained_at_or_before_current = Some(retained.len() - 1);
                    }
                    continue;
                }
                if previous_current.is_some_and(|current| position <= current) {
                    retained_at_or_before_current = Some(retained.len());
                }
                retained.push(entry);
            }
        }
        self.entries = retained;
        self.current = if self.entries.is_empty() {
            None
        } else {
            retained_at_or_before_current.or(Some(0))
        };
        self.coalesce_current();
    }

    fn coalesce_current(&mut self) {
        let Some(mut position) = self.current else {
            return;
        };
        if position > 0 && self.entries[position - 1] == self.entries[position] {
            self.entries.remove(position);
            position -= 1;
        }
        if position + 1 < self.entries.len() && self.entries[position + 1] == self.entries[position]
        {
            self.entries.remove(position + 1);
        }
        self.current = Some(position);
    }
}

#[cfg(test)]
mod tests {
    use super::{HistoryDirection, NavigationHistory, PageLocation};
    use onenote_core::{PageId, SectionId, SourceId};

    #[test]
    fn traverses_back_and_forward_without_creating_entries() {
        let mut history = NavigationHistory::default();
        history.record(location("source", "section", "one"));
        history.record(location("source", "section", "two"));
        history.record(location("source", "section", "three"));

        assert_eq!(
            history.step(HistoryDirection::Back),
            Some(location("source", "section", "two"))
        );
        assert_eq!(
            history.step(HistoryDirection::Back),
            Some(location("source", "section", "one"))
        );
        assert_eq!(history.step(HistoryDirection::Back), None);
        assert_eq!(
            history.step(HistoryDirection::Forward),
            Some(location("source", "section", "two"))
        );
        assert!(history.can_go_back());
        assert!(history.can_go_forward());
    }

    #[test]
    fn new_navigation_after_back_discards_the_forward_branch() {
        let mut history = NavigationHistory::default();
        history.record(location("source", "section", "one"));
        history.record(location("source", "section", "two"));
        history.record(location("source", "section", "three"));
        history.step(HistoryDirection::Back);

        history.record(location("source", "section", "new"));

        assert!(!history.can_go_forward());
        assert_eq!(
            history.step(HistoryDirection::Back),
            Some(location("source", "section", "two"))
        );
    }

    #[test]
    fn consecutive_duplicate_visits_are_ignored() {
        let mut history = NavigationHistory::default();
        history.record(location("source", "section", "page"));
        history.record(location("source", "section", "page"));

        assert!(!history.can_go_back());
        assert!(!history.can_go_forward());
    }

    #[test]
    fn capacity_evicts_the_oldest_entries() {
        let mut history = NavigationHistory::with_capacity(2);
        history.record(location("source", "section", "one"));
        history.record(location("source", "section", "two"));
        history.record(location("source", "section", "three"));

        assert_eq!(
            history.step(HistoryDirection::Back),
            Some(location("source", "section", "two"))
        );
        assert_eq!(history.step(HistoryDirection::Back), None);
    }

    #[test]
    fn replacing_a_provisional_page_does_not_add_history() {
        let mut history = NavigationHistory::default();
        history.replace_current(location("provisional", "section", "page"));
        history.replace_current(location("restored", "section", "page"));

        assert_eq!(
            history.current(),
            Some(&location("restored", "section", "page"))
        );
        assert!(!history.can_go_back());
    }

    #[test]
    fn replacing_with_an_adjacent_page_coalesces_duplicates() {
        let mut history = NavigationHistory::default();
        history.record(location("source", "section", "one"));
        history.record(location("source", "section", "two"));

        history.replace_current(location("source", "section", "one"));

        assert!(!history.can_go_back());
        assert_eq!(
            history.current(),
            Some(&location("source", "section", "one"))
        );
    }

    #[test]
    fn removing_a_source_selects_the_nearest_surviving_page() {
        let mut history = NavigationHistory::default();
        history.record(location("first", "section", "one"));
        history.record(location("removed", "section", "two"));
        history.record(location("third", "section", "three"));
        history.step(HistoryDirection::Back);

        history.retain(|entry| entry.source != SourceId::new("removed"));

        assert_eq!(
            history.current(),
            Some(&location("first", "section", "one"))
        );
        assert!(history.can_go_forward());
        assert_eq!(
            history.step(HistoryDirection::Forward),
            Some(location("third", "section", "three"))
        );
    }

    #[test]
    fn removing_all_entries_resets_the_cursor() {
        let mut history = NavigationHistory::default();
        history.record(location("removed", "section", "page"));

        history.retain(|_| false);

        assert_eq!(history.current(), None);
        assert!(!history.can_go_back());
        assert!(!history.can_go_forward());
    }

    #[test]
    fn pruning_an_entry_does_not_leave_adjacent_duplicates() {
        let mut history = NavigationHistory::default();
        history.record(location("source", "section", "same"));
        history.record(location("removed", "section", "middle"));
        history.record(location("source", "section", "same"));

        history.retain(|entry| entry.source != SourceId::new("removed"));

        assert_eq!(
            history.current(),
            Some(&location("source", "section", "same"))
        );
        assert!(!history.can_go_back());
    }

    fn location(source: &str, section: &str, page: &str) -> PageLocation {
        PageLocation {
            source: SourceId::new(source),
            section: SectionId::new(section),
            page: PageId::new(page),
        }
    }
}
