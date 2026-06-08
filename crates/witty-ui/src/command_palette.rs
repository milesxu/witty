use std::ops::Range;
use witty_plugin_api::CommandRegistration;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommandPalette {
    open: bool,
    query: String,
    commands: Vec<CommandRegistration>,
    filtered_indices: Vec<usize>,
    selected: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandPaletteItem<'a> {
    pub filtered_index: usize,
    pub command: &'a CommandRegistration,
    pub selected: bool,
}

impl CommandPalette {
    pub fn open(&mut self, commands: &[CommandRegistration]) {
        self.open = true;
        self.query.clear();
        self.commands = commands.to_vec();
        self.selected = 0;
        self.rebuild_filter();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.filtered_indices.clear();
        self.selected = 0;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn filtered_count(&self) -> usize {
        self.filtered_indices.len()
    }

    pub fn input_text(&mut self, text: &str) {
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

        self.query.push_str(&printable);
        self.selected = 0;
        self.rebuild_filter();
    }

    pub fn backspace(&mut self) {
        if !self.open || self.query.pop().is_none() {
            return;
        }

        self.selected = 0;
        self.rebuild_filter();
    }

    pub fn move_selection(&mut self, delta: isize) {
        if !self.open || self.filtered_indices.is_empty() {
            self.selected = 0;
            return;
        }

        let max_index = self.filtered_indices.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max_index) as usize;
    }

    pub fn selected_command(&self) -> Option<&CommandRegistration> {
        self.filtered_indices
            .get(self.selected)
            .and_then(|index| self.commands.get(*index))
    }

    pub fn selected_index(&self) -> Option<usize> {
        if self.open && !self.filtered_indices.is_empty() {
            Some(self.selected)
        } else {
            None
        }
    }

    pub fn confirm(&mut self) -> Option<String> {
        let command_id = self.selected_command().map(|command| command.id.clone());
        self.close();
        command_id
    }

    pub fn visible_items(&self, limit: usize) -> Vec<CommandPaletteItem<'_>> {
        let range = self.visible_range(limit);
        self.filtered_indices
            .iter()
            .enumerate()
            .skip(range.start)
            .take(range.len())
            .filter_map(|(filtered_index, command_index)| {
                self.commands
                    .get(*command_index)
                    .map(|command| CommandPaletteItem {
                        filtered_index,
                        command,
                        selected: filtered_index == self.selected,
                    })
            })
            .collect()
    }

    pub fn visible_range(&self, limit: usize) -> Range<usize> {
        if limit == 0 || self.filtered_indices.is_empty() {
            return 0..0;
        }

        let start = self.selected.saturating_add(1).saturating_sub(limit);
        let end = start.saturating_add(limit).min(self.filtered_indices.len());
        start..end
    }

    fn rebuild_filter(&mut self) {
        let needle = self.query.to_lowercase();
        let mut matches = self
            .commands
            .iter()
            .enumerate()
            .filter_map(|(index, command)| {
                command_match_score(command, &needle).map(|score| (index, score))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|(left_index, left_score), (right_index, right_score)| {
            right_score
                .cmp(left_score)
                .then_with(|| left_index.cmp(right_index))
        });
        self.filtered_indices = matches.into_iter().map(|(index, _score)| index).collect();
        self.selected = self
            .selected
            .min(self.filtered_indices.len().saturating_sub(1));
    }
}

fn command_match_score(command: &CommandRegistration, needle: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }

    [
        (&command.title, 300),
        (&command.id, 200),
        (&command.source_plugin, 100),
    ]
    .into_iter()
    .filter_map(|(field, bonus)| {
        field_match_score(&field.to_lowercase(), needle).map(|score| score + bonus)
    })
    .max()
}

fn field_match_score(haystack: &str, needle: &str) -> Option<i32> {
    if haystack == needle {
        return Some(30_000);
    }
    if haystack.starts_with(needle) {
        return Some(20_000 - haystack.len().saturating_sub(needle.len()) as i32);
    }
    if let Some(position) = haystack.find(needle) {
        return Some(10_000 - position as i32 * 10 - haystack.len() as i32);
    }

    fuzzy_subsequence_score(haystack, needle)
}

fn fuzzy_subsequence_score(haystack: &str, needle: &str) -> Option<i32> {
    let mut start = None;
    let mut previous = None;
    let mut gaps = 0;
    let mut haystack_chars = haystack.chars().enumerate();

    for needle_char in needle.chars() {
        let (index, _) = haystack_chars.find(|(_, haystack_char)| *haystack_char == needle_char)?;
        if start.is_none() {
            start = Some(index);
        }
        if let Some(previous) = previous {
            gaps += index.saturating_sub(previous + 1);
        }
        previous = Some(index);
    }

    Some(1_000 - start.unwrap_or_default() as i32 * 5 - gaps as i32 * 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_resets_query_and_exposes_all_commands() {
        let commands = commands();
        let mut palette = CommandPalette::default();

        palette.open(&commands);

        assert!(palette.is_open());
        assert_eq!(palette.query(), "");
        assert_eq!(palette.filtered_count(), 4);
        assert_eq!(palette.selected_command().unwrap().id, "witty.about");
    }

    #[test]
    fn query_filters_title_id_and_source_plugin() {
        let commands = commands();
        let mut palette = CommandPalette::default();

        palette.open(&commands);
        palette.input_text("fix");

        assert_eq!(palette.filtered_count(), 1);
        assert_eq!(palette.selected_command().unwrap().id, "fixture.echo");

        palette.backspace();
        palette.backspace();
        palette.backspace();
        palette.input_text("builtin");

        assert_eq!(palette.filtered_count(), 1);
        assert_eq!(palette.selected_command().unwrap().id, "witty.about");
    }

    #[test]
    fn fuzzy_query_matches_ordered_characters() {
        let commands = commands();
        let mut palette = CommandPalette::default();

        palette.open(&commands);
        palette.input_text("nw");

        assert_eq!(palette.filtered_count(), 1);
        assert_eq!(palette.selected_command().unwrap().id, "workspace.new");
    }

    #[test]
    fn query_ranks_stronger_match_first() {
        let commands = commands();
        let mut palette = CommandPalette::default();

        palette.open(&commands);
        palette.input_text("echo");

        assert_eq!(palette.filtered_count(), 2);
        assert_eq!(palette.selected_command().unwrap().id, "fixture.echo");
    }

    #[test]
    fn selection_moves_inside_filtered_results_and_confirm_closes() {
        let commands = commands();
        let mut palette = CommandPalette::default();

        palette.open(&commands);
        palette.move_selection(1);
        palette.move_selection(1);

        assert_eq!(palette.selected_index(), Some(2));
        assert_eq!(palette.selected_command().unwrap().id, "workspace.new");
        assert_eq!(palette.confirm(), Some("workspace.new".to_owned()));
        assert!(!palette.is_open());
        assert_eq!(palette.selected_index(), None);
    }

    #[test]
    fn visible_items_keep_selected_row_visible() {
        let commands = commands();
        let mut palette = CommandPalette::default();

        palette.open(&commands);
        palette.move_selection(2);
        let items = palette.visible_items(2);

        assert_eq!(items.len(), 2);
        assert_eq!(palette.visible_range(2), 1..3);
        assert_eq!(items[0].filtered_index, 1);
        assert_eq!(items[0].command.id, "fixture.echo");
        assert_eq!(items[1].filtered_index, 2);
        assert_eq!(items[1].command.id, "workspace.new");
        assert!(items[1].selected);
    }

    #[test]
    fn visible_items_slide_back_toward_start_when_selection_moves_up() {
        let commands = commands();
        let mut palette = CommandPalette::default();

        palette.open(&commands);
        palette.move_selection(3);
        assert_eq!(palette.visible_range(2), 2..4);
        assert_eq!(
            palette
                .visible_items(2)
                .iter()
                .map(|item| item.command.id.as_str())
                .collect::<Vec<_>>(),
            vec!["workspace.new", "workspace.echo_history"]
        );

        palette.move_selection(-2);
        let items = palette.visible_items(2);

        assert_eq!(palette.selected_index(), Some(1));
        assert_eq!(palette.visible_range(2), 0..2);
        assert_eq!(items[0].command.id, "witty.about");
        assert_eq!(items[1].command.id, "fixture.echo");
        assert!(items[1].selected);
    }

    #[test]
    fn no_visible_window_when_limit_is_zero_or_filter_is_empty() {
        let commands = commands();
        let mut palette = CommandPalette::default();

        palette.open(&commands);
        assert_eq!(palette.visible_range(0), 0..0);
        assert!(palette.visible_items(0).is_empty());

        palette.input_text("missing");

        assert_eq!(palette.filtered_count(), 0);
        assert_eq!(palette.selected_index(), None);
        assert_eq!(palette.visible_range(3), 0..0);
        assert!(palette.visible_items(3).is_empty());
    }

    fn commands() -> Vec<CommandRegistration> {
        vec![
            CommandRegistration {
                id: "witty.about".to_owned(),
                title: "About Witty".to_owned(),
                source_plugin: "builtin".to_owned(),
            },
            CommandRegistration {
                id: "fixture.echo".to_owned(),
                title: "Fixture Echo".to_owned(),
                source_plugin: "fixture".to_owned(),
            },
            CommandRegistration {
                id: "workspace.new".to_owned(),
                title: "New Workspace".to_owned(),
                source_plugin: "workspace".to_owned(),
            },
            CommandRegistration {
                id: "workspace.echo_history".to_owned(),
                title: "Workspace Echo History".to_owned(),
                source_plugin: "workspace".to_owned(),
            },
        ]
    }
}
