use ratatui::{
    symbols::scrollbar::Set,
    widgets::{Scrollbar, ScrollbarOrientation},
};

const VERTICAL: Set = Set { track: "│", thumb: "█", begin: "↑", end: "↓" };
const HORIZONTAL: Set = Set { track: "─", thumb: "█", begin: "←", end: "→" };

pub fn scrollbar<'a>(orientation: ScrollbarOrientation) -> Scrollbar<'a> {
    let symbols = match &orientation {
        ScrollbarOrientation::VerticalRight | ScrollbarOrientation::VerticalLeft => VERTICAL,
        ScrollbarOrientation::HorizontalTop | ScrollbarOrientation::HorizontalBottom => HORIZONTAL,
    };

    Scrollbar::new(orientation).symbols(symbols)
}
