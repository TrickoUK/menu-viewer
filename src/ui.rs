use crate::app::{App, ConfirmDialog, Popup};
use crate::model::Row;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

fn highlight_style() -> Style {
    Style::default()
        .bg(Color::Blue)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(5),
        ])
        .split(area);

    draw_title(f, chunks[0], app);
    draw_rows(f, chunks[1], app);
    draw_description(f, chunks[2], app);

    if let Some(popup) = app.popup() {
        draw_popup(f, area, popup);
    }

    if let Some(confirm) = app.confirm_dialog() {
        draw_confirm(f, area, confirm, app.enabled_diff().len());
    }
}

fn draw_title(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Batocera Menu Viewer");
    let p = Paragraph::new(app.breadcrumb()).block(block);
    f.render_widget(p, area);
}

fn draw_rows(f: &mut Frame, area: Rect, app: &App) {
    let level = app.current_level();
    let items: Vec<ListItem> = level.rows.iter().map(|row| row_item(row, app)).collect();

    let mut state = ListState::default();
    if !level.rows.is_empty() {
        state.select(Some(level.cursor));
    }

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Options"))
        .highlight_style(highlight_style());

    f.render_stateful_widget(list, area, &mut state);
}

fn row_item(row: &Row, app: &App) -> ListItem<'static> {
    // Toggle/Choice rows that are currently disabled render as a single
    // flat grey line, deliberately bypassing the normal green `[value]`
    // span below: patching a style onto the Line's own style would only
    // affect spans that don't already set their own color, and the value
    // span explicitly sets green, which would otherwise still win.
    if let Some(key) = row.key() {
        if !app.is_enabled(key) {
            let val = app.selection(key).unwrap_or("");
            let text = format!("{:<40} [{val}]", row.prompt());
            return ListItem::new(Line::from(text).style(Style::default().fg(Color::DarkGray)));
        }
    }

    let line: Line<'static> = match row {
        Row::Submenu { title, .. } => {
            Line::from(format!("{title} >")).style(Style::default().fg(Color::Cyan))
        }
        Row::GroupHeader { title } => Line::from(format!("── {title} ──")).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Row::Toggle { key, prompt, .. } | Row::Choice { key, prompt, .. } => {
            let val = app.selection(key).unwrap_or("");
            Line::from(vec![
                Span::raw(format!("{prompt:<40} ")),
                Span::styled(format!("[{val}]"), Style::default().fg(Color::Green)),
            ])
        }
        Row::Placeholder { label, note, .. } => {
            Line::from(format!("{label} {note}")).style(Style::default().add_modifier(Modifier::DIM))
        }
    };
    ListItem::new(line)
}

fn draw_description(f: &mut Frame, area: Rect, app: &App) {
    let level = app.current_level();
    let text = level
        .rows
        .get(level.cursor)
        .and_then(|r| r.description())
        .unwrap_or("");
    let p = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Description"))
        .wrap(Wrap { trim: true });
    f.render_widget(p, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn draw_popup(f: &mut Frame, area: Rect, popup: &Popup) {
    let rect = centered_rect(50, 50, area);
    f.render_widget(Clear, rect);

    let items: Vec<ListItem> = popup
        .choices
        .iter()
        .map(|(label, _)| ListItem::new(label.clone()))
        .collect();

    let mut state = ListState::default();
    state.select(Some(popup.cursor));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(popup.prompt.clone()),
        )
        .highlight_style(highlight_style());

    f.render_stateful_widget(list, rect, &mut state);
}

fn draw_confirm(f: &mut Frame, area: Rect, confirm: &ConfirmDialog, pending: usize) {
    let rect = centered_rect(50, 30, area);
    f.render_widget(Clear, rect);

    let items = vec![
        ListItem::new("Save and exit"),
        ListItem::new("Discard and exit"),
    ];

    let mut state = ListState::default();
    state.select(Some(confirm.cursor));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("{pending} pending change(s) — save to file?")),
        )
        .highlight_style(highlight_style());

    f.render_stateful_widget(list, rect, &mut state);
}
