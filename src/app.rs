use std::sync::Arc;

use futures::{Stream, StreamExt, pin_mut};
use matrix_sdk::{Client, Room};
use matrix_sdk_ui::sync_service::{self, SyncService};
use ratatui::{
    DefaultTerminal,
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};
use tokio::{
    spawn,
    sync::mpsc::{Receiver, Sender, channel},
};

use crate::{
    Error,
    input::{self, Input},
    mode, room,
    task_ext::JoinHandleExt,
};

pub enum Message {
    Quit,
    OpenRoom(Room),
    Room(room::Message),
    Mode(Mode),
    Space(mode::space::Message),
    RoomList(mode::room_list::Message),
    Logger(mode::logger::Message),
}

#[derive(Default)]
pub enum Mode {
    #[default]
    None,
    Insert,
    Space(mode::space::Model),
    RoomList(mode::room_list::Model),
    Room(mode::room::Model),
    Logger(mode::logger::Model),
}

pub struct Model {
    pub exit: bool,
    pub input_sender: Sender<Input>,
    pub client: Client,
    pub sync_service: Arc<SyncService>,
    pub mode: Mode,
    pub room: Option<room::Model>,
}

impl Model {
    pub async fn new(client: Client, input_sender: Sender<Input>) -> Result<Self, Error> {
        let sync_service = SyncService::builder(client.clone()).with_offline_mode().build().await?;
        sync_service.start().await;

        Ok(Self {
            exit: false,
            input_sender,
            client,
            sync_service: Arc::new(sync_service),
            mode: Mode::default(),
            room: None,
        })
    }

    pub async fn update(&mut self, message: Message) -> Option<Message> {
        match message {
            Message::Quit => self.exit = true,
            Message::OpenRoom(room) => {
                self.mode = Mode::None;
                self.sync_service.room_list_service().subscribe_to_rooms(&[room.room_id()]).await;
                self.room = Some(room::Model::new(room, self.input_sender.clone()).await);
            }
            Message::Room(room_message) => {
                if let Some(room_model) = &mut self.room {
                    return room_model.update(room_message).await;
                }
            }
            Message::Mode(mode) => self.mode = mode,
            Message::Space(space_message) => {
                if let Mode::Space(space_model) = &mut self.mode {
                    return space_model.update(space_message).await;
                }
            }
            Message::RoomList(room_list_message) => {
                if let Mode::RoomList(room_list_model) = &mut self.mode {
                    return room_list_model.update(room_list_message).await;
                }
            }
            Message::Logger(logger_message) => {
                if let Mode::Logger(logger_model) = &mut self.mode {
                    return logger_model.update(logger_message);
                }
            }
        }

        None
    }

    pub fn render(&mut self, area: Rect, buffer: &mut Buffer) {
        let [app_area, status_area] =
            Layout::vertical([Constraint::Percentage(100), Constraint::Length(1)]).areas(area);

        // App.
        {
            if let Mode::Logger(logger_model) = &self.mode {
                logger_model.render(app_area, buffer);
            } else if let Some(room_model) = &self.room {
                room_model.render(app_area, buffer);
            } else {
                let app_area = area.inner(Margin { horizontal: 4, vertical: 2 });
                let italic = Style::default().italic();
                let yellow = Style::default().yellow();

                Paragraph::new(vec![
                    Line::from("Welcome to ✨ multiverse ✨!").centered(),
                    Line::from(""),
                    Line::from("Please take a seat & relax").centered(),
                    Line::from(""),
                    Line::from(""),
                    Line::from(vec![
                        Span::raw("Use multiverse via "),
                        Span::styled("modes", italic),
                        Span::raw(":"),
                    ]),
                    Line::from(vec![
                        Span::raw("* Press "),
                        Span::styled("<Space>", italic),
                        Span::raw(" to activate the "),
                        Span::styled("Space", yellow),
                        Span::raw(" mode,"),
                    ]),
                    Line::from(vec![
                        Span::raw("* Press "),
                        Span::styled("<r>", italic),
                        Span::raw(" to activate the "),
                        Span::styled("Room", yellow),
                        Span::raw(" mode,"),
                    ]),
                    Line::from(vec![
                        Span::raw("* Press "),
                        Span::styled("<i>", italic),
                        Span::raw(" to activate the "),
                        Span::styled("Insert", yellow),
                        Span::raw(" mode,"),
                    ]),
                    Line::from(vec![
                        Span::raw("* Press "),
                        Span::styled("<l>", italic),
                        Span::raw(" to activate the "),
                        Span::styled("Logger", yellow),
                        Span::raw(" mode (highly experimental),"),
                    ]),
                    Line::from(vec![
                        Span::raw("* Press "),
                        Span::styled("<Esc>", italic),
                        Span::raw(" to desactivate the current mode"),
                    ]),
                    Line::from(vec![
                        Span::raw("* Press "),
                        Span::styled("<q>", italic),
                        Span::raw(" to "),
                        Span::styled("quit", yellow.add_modifier(Modifier::BOLD)),
                        Span::raw("!"),
                    ]),
                ])
                .wrap(Wrap { trim: true })
                .render(app_area, buffer);
            }
        }

        // Status
        {
            let [mode_area, sync_service_area] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(status_area);

            let (mode_label, mode_color) = match &mut self.mode {
                Mode::None => ("none", Color::Gray),
                Mode::Insert => ("insert", Color::Green),
                Mode::Space(space_model) => {
                    space_model.render(app_area, buffer);

                    ("space", Color::Gray)
                }
                Mode::RoomList(room_list_model) => {
                    room_list_model.render(app_area, buffer);

                    ("room list", Color::Gray)
                }
                Mode::Room(room_model) => {
                    room_model.render(app_area, buffer);

                    ("room", Color::Gray)
                }
                Mode::Logger(_) => ("logger", Color::Gray),
            };

            let (sync_service_label, sync_service_color) = match self.sync_service.state().get() {
                sync_service::State::Idle => ("idle", Color::Gray),
                sync_service::State::Running => ("running", Color::Green),
                sync_service::State::Terminated => ("terminated", Color::Yellow),
                sync_service::State::Error(_) => ("ERROR", Color::Red),
                sync_service::State::Offline => ("offline", Color::Blue),
            };

            Line::from(format!("mode `{}`", mode_label))
                .style(Style::new().fg(mode_color))
                .render(mode_area, buffer);

            Line::from(format!("sync service `{}`", sync_service_label))
                .style(Style::new().fg(sync_service_color))
                .right_aligned()
                .render(sync_service_area, buffer);
        }
    }
}

pub struct App {
    model: Model,
    input_receiver: Receiver<Input>,
}

impl App {
    pub async fn new(client: Client) -> Result<Self, Error> {
        let (input_sender, input_receiver) = channel(128);

        Ok(Self { model: Model::new(client, input_sender).await?, input_receiver })
    }

    pub async fn run(mut self, terminal: &mut DefaultTerminal) -> Result<(), Error> {
        let _terminal_events_task =
            spawn(input::handle_terminal_events_task(self.model.input_sender.clone()))
                .abort_on_drop();

        let _sync_service_task = spawn(handle_sync_service_states_task(
            self.model.input_sender.clone(),
            self.model.sync_service.state(),
        ))
        .abort_on_drop();

        // Run the app.
        while !self.model.exit {
            // Render the app.
            terminal.draw(|frame| self.model.render(frame.area(), frame.buffer_mut()))?;

            // Handle inputs and get a `Message` in return.
            let mut next_message =
                input::map_input_to_message(&mut self.input_receiver, &self.model).await;

            // Process the `Message` and the subsequent `Message`s if any are chained.
            while let Some(message) = next_message {
                next_message = self.model.update(message).await;
            }
        }

        self.model.sync_service.stop().await;

        Ok(())
    }
}

async fn handle_sync_service_states_task(
    input_sender: Sender<Input>,
    state_receiver: impl Stream<Item = sync_service::State>,
) {
    pin_mut!(state_receiver);

    while let Some(_state) = state_receiver.next().await {
        let _ = input_sender.send(Input::Redraw).await;
    }
}
