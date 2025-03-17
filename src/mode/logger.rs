use std::{ops::Not, time::Duration};

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style, Stylize},
    widgets::{Cell, Clear, Row, Table, Widget},
};
use tokio::{spawn, sync::mpsc::Sender, time::sleep};
use tui_logger::{
    TuiLoggerSmartWidget as LoggerWidget, TuiWidgetEvent as WidgetEvent,
    TuiWidgetState as LoggerState,
};

use crate::{
    app,
    block::{self, block_with_title},
    input::Input,
    task_ext::{AbortOnDrop, JoinHandleExt},
};

pub enum Scroll {
    Up,
    Down,
}

pub enum Message {
    OpenCommandPanel,
    Scroll(Scroll),
    ToggleFilters,
    FocusFilter,
    IncreaseShownLogLevel,
    DecreaseShownLogLevel,
}

pub struct Model {
    state: LoggerState,
    command_panel_is_opened: bool,
    filters_are_visible: bool,
    _log_updates_handle: AbortOnDrop<()>,
}

impl Model {
    pub fn new(input_sender: Sender<Input>) -> Self {
        let state = LoggerState::default();
        state.transition(WidgetEvent::HideKey);

        let _log_updates_handle = spawn(log_updates_task(input_sender)).abort_on_drop();

        Self {
            state,
            command_panel_is_opened: false,
            filters_are_visible: false,
            _log_updates_handle,
        }
    }

    pub fn update(&mut self, message: Message) -> Option<app::Message> {
        self.command_panel_is_opened = false;

        match message {
            Message::OpenCommandPanel => {
                self.command_panel_is_opened = true;
            }
            Message::Scroll(scroll) => self.state.transition(if self.filters_are_visible {
                match scroll {
                    Scroll::Up => WidgetEvent::UpKey,
                    Scroll::Down => WidgetEvent::DownKey,
                }
            } else {
                match scroll {
                    Scroll::Up => WidgetEvent::PrevPageKey,
                    Scroll::Down => WidgetEvent::NextPageKey,
                }
            }),
            Message::ToggleFilters => {
                self.filters_are_visible = self.filters_are_visible.not();
                self.state.transition(WidgetEvent::HideKey);
            }
            Message::FocusFilter => {
                if self.filters_are_visible {
                    self.state.transition(WidgetEvent::FocusKey);
                }
            }
            Message::IncreaseShownLogLevel => {
                if self.filters_are_visible {
                    self.state.transition(WidgetEvent::RightKey);
                }
            }
            Message::DecreaseShownLogLevel => {
                if self.filters_are_visible {
                    self.state.transition(WidgetEvent::LeftKey);
                }
            }
        }

        None
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        let maybe_dim = if self.filters_are_visible { Modifier::DIM } else { Modifier::empty() };

        let logger = LoggerWidget::default()
            .border_type(block::BORDER_TYPE)
            .style_error(Style::default().red().bold().add_modifier(maybe_dim))
            .style_warn(Style::default().yellow().add_modifier(maybe_dim))
            .style_info(Style::default().magenta().add_modifier(maybe_dim))
            .style_trace(Style::default().green().add_modifier(maybe_dim))
            .style_debug(Style::default().blue().add_modifier(maybe_dim))
            .title_log("Logs")
            .title_target("Filter by targets")
            .state(&self.state);

        logger.render(area, buffer);

        if self.command_panel_is_opened {
            let rows = [
                Row::new([Cell::new("f"), Cell::new("Toggle filters")]),
                Row::new([Cell::new("───"), Cell::new("Focus in on filters ─────────────")]),
                Row::new([Cell::new("s"), Cell::new("Toggle the target")]),
                Row::new([Cell::new("→"), Cell::new("Increase log level for the target")]),
                Row::new([Cell::new("←"), Cell::new("Decrease log level for the target")]),
                Row::new([Cell::new("↑"), Cell::new("Scroll the targets up")]),
                Row::new([Cell::new("↓"), Cell::new("Scroll the targets down")]),
                Row::new([Cell::new("───"), Cell::new("Focus is on logs ─────────────────")]),
                Row::new([Cell::new("↑"), Cell::new("Scroll the logs up")]),
                Row::new([Cell::new("↓"), Cell::new("Scroll the logs down")]),
            ];

            let [_, area] = Layout::vertical([
                Constraint::Percentage(100),
                Constraint::Min(rows.len() as u16 + 2),
            ])
            .areas(area);
            let [_, area] =
                Layout::horizontal([Constraint::Percentage(100), Constraint::Min(41)]).areas(area);

            Clear.render(area, buffer);

            Table::default()
                .rows(rows)
                .widths([Constraint::Length(3), Constraint::Percentage(100)])
                .block(block_with_title("Logger"))
                .render(area, buffer);
        }
    }
}

async fn log_updates_task(input_sender: Sender<Input>) {
    loop {
        sleep(Duration::from_secs(1)).await;

        if input_sender.send(Input::Redraw).await.is_err() {
            break;
        }
    }
}
