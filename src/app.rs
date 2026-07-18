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

pub struct App<'a> {
    stack: Vec<MenuLevel<'a>>,
    popup: Option<Popup<'a>>,
    selections: HashMap<String, String>,
    pub should_quit: bool,
}

impl<'a> App<'a> {
    pub fn new(root: &'a [Row]) -> Self {
        let mut selections = HashMap::new();
        init_selections(root, &mut selections);
        App {
            stack: vec![MenuLevel {
                title: "MENU".to_string(),
                rows: root,
                cursor: 0,
            }],
            popup: None,
            selections,
            should_quit: false,
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

        match code {
            KeyCode::Char('q') => self.should_quit = true,
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
                    self.should_quit = true;
                }
            }
            _ => {}
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
