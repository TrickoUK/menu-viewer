use crate::app::{App, Popup};
use crate::model::Row;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

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
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(list, area, &mut state);
}

fn row_item(row: &Row, app: &App) -> ListItem<'static> {
    let line = match row {
        Row::Submenu { title, .. } => format!("{title} >"),
        Row::Toggle { key, prompt, .. } => {
            let val = app.selection(key).unwrap_or("");
            format!("{prompt:<40} [{val}]")
        }
        Row::Choice { key, prompt, .. } => {
            let val = app.selection(key).unwrap_or("");
            format!("{prompt:<40} [{val}]")
        }
        Row::Placeholder { label, note } => format!("{label} {note}"),
    };
    let style = if matches!(row, Row::Placeholder { .. }) {
        Style::default().add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };
    ListItem::new(line).style(style)
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
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(list, rect, &mut state);
}
