//! Library search state and substring filter.
//!
//! Pure data + free function. The filter logic is decoupled from
//! LibraryApp so it can be tested in isolation. `SearchState` is the
//! container LibraryApp embeds; `filter_indices` is the work function
//! called on every keystroke.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    /// No search is active; the full entries list is shown.
    #[default]
    Idle,
    /// Search box is open and accepting keystrokes. The query updates
    /// live and the filter narrows immediately.
    Editing,
    /// Search box is closed but the filter is still in effect. Arrow
    /// keys navigate the filtered set; `/` re-opens the box for refine,
    /// Esc clears everything and returns to Idle.
    Applied,
}

#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub mode: SearchMode,
    pub query: String,
    /// Indices into the owning LibraryApp's `entries`. Only populated
    /// in Editing or Applied; LibraryApp uses `all_indices` instead
    /// when this is empty AND mode is Idle (mode disambiguates "empty
    /// because no filter" from "empty because zero matches").
    pub filtered: Vec<usize>,
}

/// Filter `haystacks` against `query`. `query` is expected to be
/// already-lowercased by the caller (saves repeated allocations).
/// Returns indices in source order. Empty query returns ALL indices
/// (so the renderer never has to special-case "filter present but
/// empty query — show what?").
pub fn filter_indices(haystacks: &[String], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..haystacks.len()).collect();
    }
    haystacks
        .iter()
        .enumerate()
        .filter_map(|(i, h)| if h.contains(query) { Some(i) } else { None })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus() -> Vec<String> {
        // Pre-lowercased title\nauthor strings, simulating the parallel
        // array that LibraryApp builds at construction time.
        vec![
            "firefly: generations\ntim lebbon".to_string(),
            "firefly: the magnificent nine\nj.m. straczynski".to_string(),
            "threshold\nclive cussler".to_string(),
            "tomorrow, and tomorrow, and tomorrow\ngabrielle zevin".to_string(),
        ]
    }

    #[test]
    fn empty_query_returns_all_indices() {
        let c = corpus();
        assert_eq!(filter_indices(&c, ""), vec![0, 1, 2, 3]);
    }

    #[test]
    fn substring_match_lowercase() {
        let c = corpus();
        assert_eq!(filter_indices(&c, "firefly"), vec![0, 1]);
    }

    #[test]
    fn no_match_returns_empty() {
        let c = corpus();
        assert!(filter_indices(&c, "zzzzzz").is_empty());
    }

    #[test]
    fn author_match_works() {
        let c = corpus();
        assert_eq!(filter_indices(&c, "lebbon"), vec![0]);
    }

    #[test]
    fn mid_word_substring_match() {
        let c = corpus();
        // "morrow" appears mid-word in "tomorrow"
        assert_eq!(filter_indices(&c, "morrow"), vec![3]);
    }

    #[test]
    fn multi_match_preserves_source_order() {
        let c = corpus();
        let r = filter_indices(&c, "fire");
        assert_eq!(r, vec![0, 1]);
    }

    #[test]
    fn title_match_works() {
        let c = corpus();
        assert_eq!(filter_indices(&c, "threshold"), vec![2]);
    }

    #[test]
    fn default_search_state_is_idle_empty() {
        let s = SearchState::default();
        assert_eq!(s.mode, SearchMode::Idle);
        assert!(s.query.is_empty());
        assert!(s.filtered.is_empty());
    }
}
