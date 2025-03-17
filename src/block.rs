use ratatui::{
    style::{Color, Style, Styled},
    widgets::{Block, BorderType, Padding},
};

pub const NO_PADDING: Padding = Padding::ZERO;
pub const PADDING: Padding = Padding::horizontal(1);
pub const BORDER_STYLE: Style = Style::new().fg(Color::Indexed(111));
pub const BORDER_TYPE: BorderType = BorderType::Rounded;

pub fn block_with_title(title: &str) -> Block {
    Block::bordered()
        .border_style(BORDER_STYLE)
        .border_type(BorderType::Rounded)
        .title(title.set_style(Style::reset()))
        .padding(PADDING)
}

pub fn block() -> Block<'static> {
    Block::bordered().border_style(BORDER_STYLE).border_type(BORDER_TYPE).padding(PADDING)
}
