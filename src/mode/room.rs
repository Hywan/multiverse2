use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    widgets::{Cell, Clear, Paragraph, Row, Table, Widget},
};

use crate::block::block_with_title;

pub struct Model {
    has_room_opened: bool,
}

impl Model {
    pub fn new(has_room_opened: bool) -> Self {
        Self { has_room_opened }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        let rows = [
            Row::new([Cell::new("b"), Cell::new("Paginate backwards")]),
            Row::new([Cell::new("r"), Cell::new("Toggle reaction to last message")]),
            Row::new([Cell::new("s"), Cell::new("Goto start of timeline")]),
            Row::new([Cell::new("e"), Cell::new("Goto end of timeline")]),
            Row::new([Cell::new("t"), Cell::new("View timeline")]),
            Row::new([Cell::new("i"), Cell::new("View event ID")]),
            Row::new([Cell::new("o"), Cell::new("View event origin")]),
            Row::new([Cell::new("l"), Cell::new("View linked chunk")]),
            Row::new([Cell::new("m"), Cell::new("Mark as read")]),
            Row::new([Cell::new("c"), Cell::new("Empty room event cache")]),
        ];

        let [_, area] = Layout::vertical([
            Constraint::Percentage(100),
            Constraint::Min(if self.has_room_opened { rows.len() as u16 } else { 1 } + 2),
        ])
        .areas(area);
        let [_, area] =
            Layout::horizontal([Constraint::Percentage(100), Constraint::Min(39)]).areas(area);

        Clear.render(area, buffer);

        let block = block_with_title("Room");

        if self.has_room_opened {
            Table::default()
                .rows(rows)
                .widths([Constraint::Length(3), Constraint::Percentage(100)])
                .block(block)
                .render(area, buffer);
        } else {
            Paragraph::new("No room opened")
                .style(Style::default().red())
                .block(block)
                .render(area, buffer);
        }
    }
}
