use crossterm::event::KeyEvent;
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use crate::block::block;

#[derive(Debug, Clone)]
pub(crate) struct TextArea {
    inner: tui_textarea::TextArea<'static>,
    has_block: bool,
}

impl TextArea {
    pub fn new() -> Self {
        Self { inner: tui_textarea::TextArea::new(vec![]), has_block: false }
    }

    pub fn new_with_border() -> Self {
        let mut new = Self::new();
        new.inner.set_block(block());
        new.has_block = true;

        new
    }

    pub fn handle_input(&mut self, key_event: KeyEvent) -> bool {
        self.inner.input(key_event)
    }

    pub fn input(&self) -> String {
        self.inner.lines().join("\n")
    }

    pub fn clear(&mut self) {
        self.inner = tui_textarea::TextArea::new(vec![]);

        if self.has_block {
            self.inner.set_block(block());
        }
    }
}

impl Widget for &TextArea {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.inner.render(area, buffer);
    }
}
