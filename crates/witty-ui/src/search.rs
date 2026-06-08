use witty_core::{try_find_search_matches, SearchError, SearchMatch, SearchOptions, SearchTextRow};
use witty_plugin_api::CommandRegistration;

pub const SEARCH_OPEN_COMMAND_ID: &str = "witty.search.open";
pub const SEARCH_CLOSE_COMMAND_ID: &str = "witty.search.close";
pub const SEARCH_NEXT_COMMAND_ID: &str = "witty.search.next";
pub const SEARCH_PREVIOUS_COMMAND_ID: &str = "witty.search.previous";
const SEARCH_HISTORY_LIMIT: usize = 32;

pub fn search_command_registrations() -> Vec<CommandRegistration> {
    [
        (SEARCH_OPEN_COMMAND_ID, "Search: Open"),
        (SEARCH_CLOSE_COMMAND_ID, "Search: Close"),
        (SEARCH_NEXT_COMMAND_ID, "Search: Next Match"),
        (SEARCH_PREVIOUS_COMMAND_ID, "Search: Previous Match"),
    ]
    .into_iter()
    .map(|(id, title)| CommandRegistration {
        id: id.to_owned(),
        title: title.to_owned(),
        source_plugin: "builtin".to_owned(),
    })
    .collect()
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalSearch {
    open: bool,
    query: String,
    options: SearchOptions,
    matches: Vec<SearchMatch>,
    active: Option<usize>,
    error: Option<SearchError>,
    history: Vec<String>,
    history_cursor: Option<usize>,
    history_draft: Option<String>,
}

impl TerminalSearch {
    pub fn open(&mut self, rows: &[SearchTextRow], selected_text: Option<&str>) {
        self.open = true;
        self.history_cursor = None;
        self.history_draft = None;
        self.query = selected_text
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| self.history.last().cloned())
            .unwrap_or_default();
        self.rebuild(rows);
    }

    pub fn close(&mut self) {
        self.commit_current_query();
        self.open = false;
        self.query.clear();
        self.matches.clear();
        self.active = None;
        self.error = None;
        self.history_cursor = None;
        self.history_draft = None;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn options(&self) -> SearchOptions {
        self.options
    }

    pub fn matches(&self) -> &[SearchMatch] {
        &self.matches
    }

    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    pub fn error(&self) -> Option<&SearchError> {
        self.error.as_ref()
    }

    pub fn error_text(&self) -> Option<String> {
        self.error
            .as_ref()
            .map(|err| err.to_string().replace('\n', " "))
    }

    pub fn active_match(&self) -> Option<SearchMatch> {
        self.active
            .and_then(|index| self.matches.get(index).copied())
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    pub fn input_text(&mut self, rows: &[SearchTextRow], text: &str) {
        if !self.open {
            return;
        }

        let printable = text
            .chars()
            .filter(|ch| !ch.is_control())
            .collect::<String>();
        if printable.is_empty() {
            return;
        }

        self.reset_history_cursor();
        self.query.push_str(&printable);
        self.rebuild(rows);
    }

    pub fn set_query(&mut self, rows: &[SearchTextRow], query: impl Into<String>) {
        if !self.open {
            return;
        }

        self.reset_history_cursor();
        self.query = query.into();
        self.rebuild(rows);
    }

    pub fn backspace(&mut self, rows: &[SearchTextRow]) {
        if !self.open || self.query.pop().is_none() {
            return;
        }

        self.reset_history_cursor();
        self.rebuild(rows);
    }

    pub fn set_case_sensitive(&mut self, rows: &[SearchTextRow], case_sensitive: bool) {
        if self.options.case_sensitive == case_sensitive {
            return;
        }

        self.options.case_sensitive = case_sensitive;
        if self.open {
            self.rebuild(rows);
        }
    }

    pub fn set_regex(&mut self, rows: &[SearchTextRow], regex: bool) {
        if self.options.regex == regex {
            return;
        }

        self.options.regex = regex;
        if self.open {
            self.rebuild(rows);
        }
    }

    pub fn toggle_regex(&mut self, rows: &[SearchTextRow]) {
        self.set_regex(rows, !self.options.regex);
    }

    pub fn set_whole_word(&mut self, rows: &[SearchTextRow], whole_word: bool) {
        if self.options.whole_word == whole_word {
            return;
        }

        self.options.whole_word = whole_word;
        if self.open {
            self.rebuild(rows);
        }
    }

    pub fn toggle_whole_word(&mut self, rows: &[SearchTextRow]) {
        self.set_whole_word(rows, !self.options.whole_word);
    }

    pub fn set_normalize_nfc(&mut self, rows: &[SearchTextRow], normalize_nfc: bool) {
        if self.options.normalize_nfc == normalize_nfc {
            return;
        }

        self.options.normalize_nfc = normalize_nfc;
        if self.open {
            self.rebuild(rows);
        }
    }

    pub fn toggle_normalize_nfc(&mut self, rows: &[SearchTextRow]) {
        self.set_normalize_nfc(rows, !self.options.normalize_nfc);
    }

    pub fn toggle_case_sensitive(&mut self, rows: &[SearchTextRow]) {
        self.set_case_sensitive(rows, !self.options.case_sensitive);
    }

    pub fn rebuild(&mut self, rows: &[SearchTextRow]) {
        if !self.open {
            return;
        }

        match try_find_search_matches(rows, &self.query, self.options) {
            Ok(matches) => {
                self.matches = matches;
                self.active = if self.matches.is_empty() {
                    None
                } else {
                    Some(0)
                };
                self.error = None;
            }
            Err(err) => {
                self.matches.clear();
                self.active = None;
                self.error = Some(err);
            }
        }
    }

    pub fn next_match(&mut self) -> Option<SearchMatch> {
        self.commit_current_query();
        self.move_active(1)
    }

    pub fn previous_match(&mut self) -> Option<SearchMatch> {
        self.commit_current_query();
        self.move_active(-1)
    }

    pub fn repeat_next(&mut self, rows: &[SearchTextRow]) -> Option<SearchMatch> {
        if self.open {
            return self.next_match();
        }

        self.open_last_history_query(rows)?;
        self.active_match()
    }

    pub fn repeat_previous(&mut self, rows: &[SearchTextRow]) -> Option<SearchMatch> {
        if self.open {
            return self.previous_match();
        }

        self.open_last_history_query(rows)?;
        self.previous_match()
    }

    pub fn previous_history_query(&mut self, rows: &[SearchTextRow]) -> Option<&str> {
        if !self.open || self.history.is_empty() {
            return None;
        }

        let next_index = self
            .history_cursor
            .map(|index| index.saturating_sub(1))
            .unwrap_or_else(|| {
                self.history_draft = Some(self.query.clone());
                self.history.len() - 1
            });
        self.apply_history_query(rows, next_index);
        Some(self.query())
    }

    pub fn next_history_query(&mut self, rows: &[SearchTextRow]) -> Option<&str> {
        let cursor = self.history_cursor?;
        if cursor + 1 < self.history.len() {
            self.apply_history_query(rows, cursor + 1);
        } else {
            self.history_cursor = None;
            self.query = self.history_draft.take().unwrap_or_default();
            self.rebuild(rows);
        }
        Some(self.query())
    }

    fn move_active(&mut self, delta: isize) -> Option<SearchMatch> {
        if self.matches.is_empty() {
            self.active = None;
            return None;
        }

        let current = self.active.unwrap_or(0) as isize;
        let len = self.matches.len() as isize;
        let next = (current + delta).rem_euclid(len) as usize;
        self.active = Some(next);
        self.active_match()
    }

    fn open_last_history_query(&mut self, rows: &[SearchTextRow]) -> Option<()> {
        let query = self.history.last()?.clone();
        self.open = true;
        self.history_cursor = None;
        self.history_draft = None;
        self.query = query;
        self.rebuild(rows);
        Some(())
    }

    fn apply_history_query(&mut self, rows: &[SearchTextRow], index: usize) {
        self.history_cursor = Some(index);
        self.query = self.history[index].clone();
        self.rebuild(rows);
    }

    fn reset_history_cursor(&mut self) {
        self.history_cursor = None;
        self.history_draft = None;
    }

    fn commit_current_query(&mut self) {
        if self.query.is_empty() || self.error.is_some() {
            return;
        }

        if let Some(existing) = self.history.iter().position(|stored| stored == &self.query) {
            self.history.remove(existing);
        }
        self.history.push(self.query.clone());
        if self.history.len() > SEARCH_HISTORY_LIMIT {
            let overflow = self.history.len() - SEARCH_HISTORY_LIMIT;
            self.history.drain(0..overflow);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use witty_core::{SearchRowId, SearchRowKind};

    #[test]
    fn open_seeds_query_and_builds_matches() {
        let rows = rows();
        let mut search = TerminalSearch::default();

        search.open(&rows, Some("error"));

        assert!(search.is_open());
        assert_eq!(search.query(), "error");
        assert_eq!(search.match_count(), 2);
        assert_eq!(search.active_index(), Some(0));
        assert_eq!(
            search.active_match(),
            Some(SearchMatch {
                row: SearchRowId::scrollback(0),
                start_col: 0,
                end_col: 4,
            })
        );
    }

    #[test]
    fn query_editing_rebuilds_matches_and_resets_active_match() {
        let rows = rows();
        let mut search = TerminalSearch::default();

        search.open(&rows, None);
        search.input_text(&rows, "warn");
        assert_eq!(search.match_count(), 1);
        assert_eq!(search.active_index(), Some(0));

        search.set_query(&rows, "error");
        search.next_match();
        assert_eq!(search.active_index(), Some(1));

        search.backspace(&rows);
        assert_eq!(search.query(), "erro");
        assert_eq!(search.match_count(), 2);
        assert_eq!(search.active_index(), Some(0));
    }

    #[test]
    fn navigation_wraps_over_matches() {
        let rows = rows();
        let mut search = TerminalSearch::default();

        search.open(&rows, Some("error"));

        assert_eq!(search.next_match().unwrap().row, SearchRowId::screen(1));
        assert_eq!(search.next_match().unwrap().row, SearchRowId::scrollback(0));
        assert_eq!(search.previous_match().unwrap().row, SearchRowId::screen(1));
    }

    #[test]
    fn close_commits_valid_queries_to_bounded_history() {
        let rows = rows();
        let mut search = TerminalSearch::default();

        search.open(&rows, None);
        for index in 0..40 {
            search.set_query(&rows, format!("query-{index}"));
            search.close();
            search.open(&rows, None);
        }

        assert_eq!(search.history_len(), SEARCH_HISTORY_LIMIT);
        assert_eq!(search.query(), "query-39");

        search.set_query(&rows, "[");
        search.toggle_regex(&rows);
        search.close();
        search.open(&rows, None);

        assert_eq!(search.query(), "query-39");
        assert_eq!(search.history_len(), SEARCH_HISTORY_LIMIT);
    }

    #[test]
    fn history_navigation_preserves_in_progress_query() {
        let rows = rows();
        let mut search = TerminalSearch::default();

        search.open(&rows, Some("error"));
        search.close();
        search.open(&rows, Some("warn"));
        search.close();

        search.open(&rows, None);
        search.input_text(&rows, "-draft");
        assert_eq!(search.previous_history_query(&rows), Some("warn"));
        assert_eq!(search.previous_history_query(&rows), Some("error"));
        assert_eq!(search.previous_history_query(&rows), Some("error"));
        assert_eq!(search.next_history_query(&rows), Some("warn"));
        assert_eq!(search.next_history_query(&rows), Some("warn-draft"));
        assert_eq!(search.next_history_query(&rows), None);
    }

    #[test]
    fn repeat_find_reopens_last_query_after_close() {
        let rows = rows();
        let mut search = TerminalSearch::default();

        search.open(&rows, Some("error"));
        search.close();

        assert_eq!(
            search.repeat_next(&rows),
            Some(SearchMatch {
                row: SearchRowId::scrollback(0),
                start_col: 0,
                end_col: 4,
            })
        );
        assert!(search.is_open());
        search.close();

        assert_eq!(
            search.repeat_previous(&rows),
            Some(SearchMatch {
                row: SearchRowId::screen(1),
                start_col: 0,
                end_col: 4,
            })
        );
    }

    #[test]
    fn case_sensitive_option_rebuilds_when_open() {
        let rows = rows();
        let mut search = TerminalSearch::default();

        search.open(&rows, Some("error"));
        assert_eq!(search.match_count(), 2);

        search.set_case_sensitive(&rows, true);
        assert_eq!(
            search.options(),
            SearchOptions {
                case_sensitive: true,
                ..SearchOptions::default()
            }
        );
        assert_eq!(search.match_count(), 1);
        assert_eq!(
            search.active_match().unwrap(),
            SearchMatch {
                row: SearchRowId::screen(1),
                start_col: 0,
                end_col: 4,
            }
        );
    }

    #[test]
    fn empty_query_and_close_clear_highlights() {
        let rows = rows();
        let mut search = TerminalSearch::default();

        search.open(&rows, Some("error"));
        search.set_query(&rows, "");
        assert_eq!(search.match_count(), 0);
        assert_eq!(search.active_match(), None);

        search.input_text(&rows, "warn");
        assert_eq!(search.match_count(), 1);
        search.close();

        assert!(!search.is_open());
        assert_eq!(search.query(), "");
        assert_eq!(search.matches(), &[]);
        assert_eq!(search.active_match(), None);
        assert_eq!(search.error(), None);
    }

    #[test]
    fn closed_search_ignores_query_edits() {
        let rows = rows();
        let mut search = TerminalSearch::default();

        search.input_text(&rows, "error");
        search.set_query(&rows, "warn");
        search.backspace(&rows);

        assert_eq!(search.query(), "");
        assert_eq!(search.match_count(), 0);
    }

    #[test]
    fn regex_and_whole_word_options_rebuild_matches() {
        let rows = vec![SearchTextRow {
            id: SearchRowId::screen(0),
            visible_row: Some(0),
            text: "ERR42 err7 ferr8".to_owned(),
            columns: Vec::new(),
        }];
        let mut search = TerminalSearch::default();

        search.open(&rows, None);
        search.input_text(&rows, r"err\d+");
        assert_eq!(search.match_count(), 0);

        search.toggle_regex(&rows);
        assert!(search.options().regex);
        assert_eq!(search.match_count(), 3);

        search.toggle_whole_word(&rows);
        assert!(search.options().whole_word);
        assert_eq!(search.match_count(), 2);

        search.toggle_case_sensitive(&rows);
        assert!(search.options().case_sensitive);
        assert_eq!(search.match_count(), 1);
        assert_eq!(search.error(), None);
    }

    #[test]
    fn normalize_nfc_option_rebuilds_literal_matches() {
        let rows = vec![
            SearchTextRow::with_columns(
                SearchRowId::screen(0),
                Some(0),
                "e\u{0301}cho",
                vec![
                    witty_core::SearchTextColumn::new(0, 0),
                    witty_core::SearchTextColumn::new(0, 0),
                    witty_core::SearchTextColumn::new(1, 1),
                    witty_core::SearchTextColumn::new(2, 2),
                    witty_core::SearchTextColumn::new(3, 3),
                ],
            ),
            SearchTextRow::new(SearchRowId::screen(1), Some(1), "\u{00e9}cho"),
        ];
        let mut search = TerminalSearch::default();

        search.open(&rows, Some("\u{00e9}"));
        assert_eq!(search.match_count(), 1);

        search.toggle_normalize_nfc(&rows);
        assert!(search.options().normalize_nfc);
        assert_eq!(search.match_count(), 2);

        search.toggle_normalize_nfc(&rows);
        assert!(!search.options().normalize_nfc);
        assert_eq!(search.match_count(), 1);
    }

    #[test]
    fn invalid_regex_sets_error_and_clears_when_fixed_or_literal() {
        let rows = rows();
        let mut search = TerminalSearch::default();

        search.open(&rows, None);
        search.toggle_regex(&rows);
        search.input_text(&rows, "[");

        assert_eq!(search.match_count(), 0);
        assert_eq!(search.active_match(), None);
        assert!(search.error_text().unwrap().contains("invalid regex"));

        search.set_query(&rows, "error");
        assert_eq!(search.error(), None);
        assert_eq!(search.match_count(), 2);

        search.set_query(&rows, "[");
        assert!(search.error().is_some());
        search.toggle_regex(&rows);
        assert_eq!(search.error(), None);
        assert_eq!(search.match_count(), 0);
    }

    #[test]
    fn search_command_registrations_are_builtin_and_content_free() {
        let commands = search_command_registrations();

        assert_eq!(
            commands
                .iter()
                .map(|command| command.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                SEARCH_OPEN_COMMAND_ID,
                SEARCH_CLOSE_COMMAND_ID,
                SEARCH_NEXT_COMMAND_ID,
                SEARCH_PREVIOUS_COMMAND_ID,
            ]
        );
        assert!(commands
            .iter()
            .all(|command| command.source_plugin == "builtin"));
        assert!(commands.iter().all(|command| {
            !command.title.contains("query")
                && !command.title.contains("match")
                && !command.id.contains("export")
        }));
    }

    fn rows() -> Vec<SearchTextRow> {
        vec![
            SearchTextRow {
                id: SearchRowId {
                    kind: SearchRowKind::Scrollback,
                    index: 0,
                },
                visible_row: None,
                text: "Error: first".to_owned(),
                columns: Vec::new(),
            },
            SearchTextRow {
                id: SearchRowId::screen(0),
                visible_row: Some(0),
                text: "warning".to_owned(),
                columns: Vec::new(),
            },
            SearchTextRow {
                id: SearchRowId::screen(1),
                visible_row: Some(1),
                text: "error: second".to_owned(),
                columns: Vec::new(),
            },
        ]
    }
}
