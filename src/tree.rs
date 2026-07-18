use crate::model::Row;
use crossterm::style::Stylize;

/// Dump a menu tree as plain text to stdout, for non-interactive validation
/// (`--tree`). Deliberately separate from `ui.rs`, which is a `Frame`-driven
/// renderer of live `App` state — this is a one-shot, stateless print of the
/// parsed tree, so it never touches the terminal (no raw mode, no alt
/// screen), keeping the output pipeable.
pub fn print_tree(rows: &[Row]) {
    println!("MENU");
    print!("{}", render_rows(rows, ""));
}

fn render_rows(rows: &[Row], prefix: &str) -> String {
    let mut out = String::new();
    let last_idx = rows.len().saturating_sub(1);
    for (i, row) in rows.iter().enumerate() {
        let is_last = i == last_idx;
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
        out.push_str(&render_row(row, prefix, connector, &child_prefix));
    }
    out
}

fn render_row(row: &Row, prefix: &str, connector: &str, child_prefix: &str) -> String {
    match row {
        Row::Submenu { title, rows } => {
            let mut s = format!("{prefix}{connector}{}\n", format!("{title} >").cyan());
            s.push_str(&render_rows(rows, child_prefix));
            s
        }
        Row::GroupHeader { title } => {
            format!("{prefix}{connector}{}\n", format!("── {title} ──").yellow().bold())
        }
        Row::Toggle {
            prompt, choices, ..
        } => format!(
            "{prefix}{connector}{prompt}  {}\n",
            format_choices(&choices[..])
        ),
        Row::Choice {
            prompt, choices, ..
        } => format!("{prefix}{connector}{prompt}  {}\n", format_choices(choices)),
        Row::Placeholder { label, note } => {
            format!("{prefix}{connector}{}\n", format!("{label} {note}").dim())
        }
    }
}

/// Joins choice labels with `/`, bracketing and greening the first (the
/// default `App::init_selections` always picks) to mirror the `[value]`
/// styling `ui.rs::row_item` uses for the current selection.
fn format_choices(choices: &[(String, String)]) -> String {
    choices
        .iter()
        .enumerate()
        .map(|(i, (label, _))| {
            if i == 0 {
                format!("[{label}]").green().to_string()
            } else {
                label.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choices(labels: &[&str]) -> Vec<(String, String)> {
        labels
            .iter()
            .map(|l| (l.to_string(), l.to_lowercase()))
            .collect()
    }

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut in_escape = false;
        for ch in s.chars() {
            if ch == '\u{1b}' {
                in_escape = true;
            } else if in_escape {
                if ch == 'm' {
                    in_escape = false;
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    #[test]
    fn format_choices_brackets_only_the_first() {
        let c = choices(&["Off", "On"]);
        let s = strip_ansi(&format_choices(&c));
        assert_eq!(s, "[Off] / On");
    }

    #[test]
    fn format_choices_joins_multiple() {
        let c = choices(&["Auto", "1x", "2x", "4x"]);
        let s = strip_ansi(&format_choices(&c));
        assert_eq!(s, "[Auto] / 1x / 2x / 4x");
    }

    #[test]
    fn tree_structure_uses_connectors_headers_and_nesting() {
        let rows = vec![
            Row::GroupHeader {
                title: "ADVANCED".to_string(),
            },
            Row::Submenu {
                title: "TIMING".to_string(),
                rows: vec![Row::Toggle {
                    key: "k".to_string(),
                    prompt: "FRAMESKIP".to_string(),
                    description: None,
                    choices: [
                        ("Off".to_string(), "off".to_string()),
                        ("On".to_string(), "on".to_string()),
                    ],
                }],
            },
            Row::Placeholder {
                label: "SLIDER".to_string(),
                note: "(preset-based option, not shown)",
            },
        ];

        let out = strip_ansi(&render_rows(&rows, ""));
        let lines: Vec<&str> = out.lines().collect();

        assert_eq!(lines[0], "├── ── ADVANCED ──");
        assert_eq!(lines[1], "├── TIMING >");
        assert_eq!(lines[2], "│   └── FRAMESKIP  [Off] / On");
        assert_eq!(lines[3], "└── SLIDER (preset-based option, not shown)");
    }

    #[test]
    fn last_row_uses_corner_connector() {
        let rows = vec![
            Row::Placeholder {
                label: "A".to_string(),
                note: "n",
            },
            Row::Placeholder {
                label: "B".to_string(),
                note: "n",
            },
        ];
        let out = strip_ansi(&render_rows(&rows, ""));
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].starts_with("├── "));
        assert!(lines[1].starts_with("└── "));
    }
}
