use crate::model::Row;
use crossterm::event::KeyCode;
use std::collections::HashMap;

pub struct MenuLevel<'a> {
    pub title: String,
    pub rows: &'a [Row],
    pub cursor: usize,
}

pub struct Popup<'a> {
    pub prompt: String,
    pub key: String,
    pub choices: &'a [(String, String)],
    pub cursor: usize,
}

/// The exit confirmation dialog: shown when quitting (via `q` or
/// Esc/Backspace at the top-level menu) would otherwise discard staged
/// enable/disable changes. `cursor` selects between "save and exit" (0)
/// and "discard and exit" (1); `App` never touches the filesystem itself —
/// it only records the user's choice in `write_requested` for `main.rs` to
/// act on after the event loop ends.
pub struct ConfirmDialog {
    pub cursor: usize,
}

pub struct App<'a> {
    stack: Vec<MenuLevel<'a>>,
    popup: Option<Popup<'a>>,
    selections: HashMap<String, String>,
    enabled: HashMap<String, bool>,
    initial_enabled: HashMap<String, bool>,
    confirm: Option<ConfirmDialog>,
    pub should_quit: bool,
    write_requested: bool,
}

impl<'a> App<'a> {
    pub fn new(root: &'a [Row]) -> Self {
        let mut selections = HashMap::new();
        init_selections(root, &mut selections);
        let mut enabled = HashMap::new();
        init_enabled(root, &mut enabled);
        let initial_enabled = enabled.clone();
        App {
            stack: vec![MenuLevel {
                title: "MENU".to_string(),
                rows: root,
                cursor: 0,
            }],
            popup: None,
            selections,
            enabled,
            initial_enabled,
            confirm: None,
            should_quit: false,
            write_requested: false,
        }
    }

    pub fn current_level(&self) -> &MenuLevel<'a> {
        self.stack.last().unwrap()
    }

    pub fn popup(&self) -> Option<&Popup<'a>> {
        self.popup.as_ref()
    }

    pub fn breadcrumb(&self) -> String {
        self.stack
            .iter()
            .map(|l| l.title.as_str())
            .collect::<Vec<_>>()
            .join(" > ")
    }

    pub fn selection(&self, key: &str) -> Option<&str> {
        self.selections.get(key).map(|s| s.as_str())
    }

    /// Live enabled/disabled state for a `custom_features` key, reflecting
    /// any Space toggles made this session — separate from the row tree's
    /// own baked `enabled` field, exactly parallel to how `selection`
    /// already overrides each row's baked default choice.
    pub fn is_enabled(&self, key: &str) -> bool {
        self.enabled.get(key).copied().unwrap_or(true)
    }

    pub fn confirm_dialog(&self) -> Option<&ConfirmDialog> {
        self.confirm.as_ref()
    }

    pub fn write_requested(&self) -> bool {
        self.write_requested
    }

    /// Keys whose enabled state has changed since load, for `main.rs` to
    /// apply to the source file if a write was requested.
    pub fn enabled_diff(&self) -> Vec<(String, bool)> {
        self.enabled
            .iter()
            .filter(|(k, v)| self.initial_enabled.get(k.as_str()) != Some(*v))
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    pub fn handle_key(&mut self, code: KeyCode) {
        if let Some(popup) = &mut self.popup {
            match code {
                KeyCode::Up => {
                    if popup.cursor > 0 {
                        popup.cursor -= 1;
                    }
                }
                KeyCode::Down => {
                    if popup.cursor + 1 < popup.choices.len() {
                        popup.cursor += 1;
                    }
                }
                KeyCode::Enter => {
                    let (_, value) = &popup.choices[popup.cursor];
                    self.selections.insert(popup.key.clone(), value.clone());
                    self.popup = None;
                }
                KeyCode::Esc => {
                    self.popup = None;
                }
                _ => {}
            }
            return;
        }

        if let Some(confirm) = &mut self.confirm {
            match code {
                KeyCode::Up => {
                    if confirm.cursor > 0 {
                        confirm.cursor -= 1;
                    }
                }
                KeyCode::Down => {
                    if confirm.cursor + 1 < 2 {
                        confirm.cursor += 1;
                    }
                }
                KeyCode::Enter => {
                    self.write_requested = confirm.cursor == 0;
                    self.confirm = None;
                    self.should_quit = true;
                }
                KeyCode::Esc => {
                    // Cancel the dialog and return to the menu; the
                    // pending change is untouched and not yet written.
                    self.confirm = None;
                }
                _ => {}
            }
            return;
        }

        match code {
            KeyCode::Char('q') => self.request_exit(),
            KeyCode::Char(' ') => self.toggle_current_row_enabled(),
            KeyCode::Up => {
                let level = self.stack.last_mut().unwrap();
                if level.cursor > 0 {
                    level.cursor -= 1;
                }
            }
            KeyCode::Down => {
                let level = self.stack.last_mut().unwrap();
                if level.cursor + 1 < level.rows.len() {
                    level.cursor += 1;
                }
            }
            KeyCode::Enter => self.activate(),
            KeyCode::Esc | KeyCode::Backspace => {
                if self.stack.len() > 1 {
                    self.stack.pop();
                } else {
                    self.request_exit();
                }
            }
            _ => {}
        }
    }

    /// Quit immediately if nothing has changed; otherwise stage a confirm
    /// dialog so a habitual `q`/Esc can't silently discard staged edits.
    fn request_exit(&mut self) {
        if self.enabled != self.initial_enabled {
            self.confirm = Some(ConfirmDialog { cursor: 0 });
        } else {
            self.should_quit = true;
        }
    }

    /// Space toggles the enabled state of the currently focused row, if
    /// it's backed by a toggleable key — a no-op on GroupHeader, Submenu,
    /// and Placeholder rows for free, since `Row::key()` returns `None`
    /// for those.
    fn toggle_current_row_enabled(&mut self) {
        let level = self.stack.last().unwrap();
        if let Some(row) = level.rows.get(level.cursor) {
            if let Some(key) = row.key() {
                let current = self.is_enabled(key);
                self.enabled.insert(key.to_string(), !current);
            }
        }
    }

    fn activate(&mut self) {
        let level = self.stack.last().unwrap();
        let rows = level.rows;
        let cursor = level.cursor;
        if rows.is_empty() {
            return;
        }
        let row = &rows[cursor];

        match row {
            Row::Submenu { title, rows } => {
                self.stack.push(MenuLevel {
                    title: title.clone(),
                    rows: rows.as_slice(),
                    cursor: 0,
                });
            }
            Row::Toggle { key, choices, .. } => {
                let current = self.selections.get(key.as_str()).cloned();
                let next = if current.as_deref() == Some(choices[0].1.as_str()) {
                    choices[1].1.clone()
                } else {
                    choices[0].1.clone()
                };
                self.selections.insert(key.clone(), next);
            }
            Row::Choice {
                key,
                prompt,
                choices,
                ..
            } => {
                let current = self.selections.get(key.as_str()).cloned();
                let cursor = choices
                    .iter()
                    .position(|(_, v)| Some(v.as_str()) == current.as_deref())
                    .unwrap_or(0);
                self.popup = Some(Popup {
                    prompt: prompt.clone(),
                    key: key.clone(),
                    choices: choices.as_slice(),
                    cursor,
                });
            }
            Row::GroupHeader { .. } => {}
            Row::Placeholder { .. } => {}
        }
    }
}

fn init_selections(rows: &[Row], selections: &mut HashMap<String, String>) {
    for row in rows {
        match row {
            Row::Toggle { key, choices, .. } => {
                selections.insert(key.clone(), choices[0].1.clone());
            }
            Row::Choice { key, choices, .. } => {
                selections.insert(key.clone(), choices[0].1.clone());
            }
            Row::Submenu { rows, .. } => init_selections(rows, selections),
            Row::GroupHeader { .. } => {}
            Row::Placeholder { .. } => {}
        }
    }
}

/// Seed the live enabled-state map from each Toggle/Choice row's baked
/// initial state (as scanned from the source file). Placeholder rows are
/// excluded on purpose: Space never acts on them, so their baked state
/// never needs session-mutable tracking.
fn init_enabled(rows: &[Row], enabled: &mut HashMap<String, bool>) {
    for row in rows {
        match row {
            Row::Toggle {
                key,
                enabled: initial,
                ..
            }
            | Row::Choice {
                key,
                enabled: initial,
                ..
            } => {
                enabled.insert(key.clone(), *initial);
            }
            Row::Submenu { rows, .. } => init_enabled(rows, enabled),
            Row::GroupHeader { .. } | Row::Placeholder { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toggle_row(key: &str) -> Row {
        Row::Toggle {
            key: key.to_string(),
            prompt: "TOGGLE".to_string(),
            description: None,
            choices: [
                ("Off".to_string(), "off".to_string()),
                ("On".to_string(), "on".to_string()),
            ],
            enabled: true,
        }
    }

    fn choice_row(key: &str) -> Row {
        Row::Choice {
            key: key.to_string(),
            prompt: "CHOICE".to_string(),
            description: None,
            choices: vec![
                ("A".to_string(), "a".to_string()),
                ("B".to_string(), "b".to_string()),
                ("C".to_string(), "c".to_string()),
            ],
            enabled: true,
        }
    }

    #[test]
    fn space_toggles_enabled_state_of_toggle_row() {
        let root = vec![toggle_row("k")];
        let mut app = App::new(&root);
        assert!(app.is_enabled("k"));

        app.handle_key(KeyCode::Char(' '));
        assert!(!app.is_enabled("k"));

        app.handle_key(KeyCode::Char(' '));
        assert!(app.is_enabled("k"));
    }

    #[test]
    fn space_does_nothing_on_group_header_submenu_or_placeholder_rows() {
        let root = vec![
            Row::GroupHeader {
                title: "G".to_string(),
            },
            Row::Submenu {
                title: "S".to_string(),
                rows: vec![],
            },
            Row::Placeholder {
                label: "L".to_string(),
                note: "n",
                enabled: true,
            },
        ];
        let mut app = App::new(&root);

        for _ in 0..root.len() {
            app.handle_key(KeyCode::Char(' '));
            app.handle_key(KeyCode::Down);
        }

        assert!(app.enabled_diff().is_empty());
    }

    #[test]
    fn space_is_ignored_while_popup_is_open() {
        let root = vec![choice_row("k")];
        let mut app = App::new(&root);
        app.handle_key(KeyCode::Enter);
        assert!(app.popup().is_some());

        app.handle_key(KeyCode::Char(' '));
        assert!(app.is_enabled("k"));
        assert!(app.popup().is_some());
    }

    #[test]
    fn q_at_top_level_with_no_changes_quits_immediately() {
        let root = vec![toggle_row("k")];
        let mut app = App::new(&root);
        app.handle_key(KeyCode::Char('q'));
        assert!(app.should_quit);
        assert!(app.confirm_dialog().is_none());
    }

    #[test]
    fn q_with_pending_changes_opens_confirm_dialog_instead_of_quitting() {
        let root = vec![toggle_row("k")];
        let mut app = App::new(&root);
        app.handle_key(KeyCode::Char(' '));
        app.handle_key(KeyCode::Char('q'));
        assert!(!app.should_quit);
        assert!(app.confirm_dialog().is_some());
    }

    #[test]
    fn confirm_dialog_yes_sets_write_requested_and_quits() {
        let root = vec![toggle_row("k")];
        let mut app = App::new(&root);
        app.handle_key(KeyCode::Char(' '));
        app.handle_key(KeyCode::Char('q'));
        app.handle_key(KeyCode::Enter); // cursor defaults to 0 = save

        assert!(app.should_quit);
        assert!(app.write_requested());
        assert_eq!(app.enabled_diff(), vec![("k".to_string(), false)]);
    }

    #[test]
    fn confirm_dialog_no_discards_and_quits() {
        let root = vec![toggle_row("k")];
        let mut app = App::new(&root);
        app.handle_key(KeyCode::Char(' '));
        app.handle_key(KeyCode::Char('q'));
        app.handle_key(KeyCode::Down); // cursor -> 1 = discard
        app.handle_key(KeyCode::Enter);

        assert!(app.should_quit);
        assert!(!app.write_requested());
    }

    #[test]
    fn confirm_dialog_esc_cancels_and_returns_to_menu() {
        let root = vec![toggle_row("k")];
        let mut app = App::new(&root);
        app.handle_key(KeyCode::Char(' '));
        app.handle_key(KeyCode::Char('q'));
        assert!(app.confirm_dialog().is_some());

        app.handle_key(KeyCode::Esc);
        assert!(app.confirm_dialog().is_none());
        assert!(!app.should_quit);
        // The pending change survived the cancelled dialog.
        assert!(!app.is_enabled("k"));

        app.handle_key(KeyCode::Char('q'));
        assert!(app.confirm_dialog().is_some());
    }

    #[test]
    fn esc_at_submenu_depth_pops_stack_without_confirm_even_with_pending_changes() {
        let sub_rows = vec![toggle_row("k")];
        let root = vec![Row::Submenu {
            title: "SUB".to_string(),
            rows: sub_rows,
        }];
        let mut app = App::new(&root);

        app.handle_key(KeyCode::Enter); // drill into submenu
        app.handle_key(KeyCode::Char(' ')); // toggle the nested row
        assert!(!app.is_enabled("k"));

        app.handle_key(KeyCode::Esc); // pop back to top level
        assert!(!app.should_quit);
        assert!(app.confirm_dialog().is_none());
        assert_eq!(app.current_level().title, "MENU");
    }
}
