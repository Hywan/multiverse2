use std::sync::Arc;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind};
use futures::{FutureExt, StreamExt};
use matrix_sdk_ui::{eyeball_im::VectorDiff, room_list_service, timeline as sdk_timeline};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::{app, mode, room, timeline};

#[derive(Debug)]
pub enum Input {
    Redraw,
    KeyPress(KeyEvent),
    RoomListUpdate(Vec<VectorDiff<room_list_service::Room>>),
    TimelineUpdate(Vec<VectorDiff<Arc<sdk_timeline::TimelineItem>>>),
}

pub async fn handle_terminal_events_task(input_sender: Sender<Input>) {
    let mut event_reader = EventStream::new();

    loop {
        match event_reader.next().fuse().await {
            Some(Ok(event)) => match event {
                Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                    let _ = input_sender.send(Input::KeyPress(key_event)).await;
                }

                Event::Resize(..) => {
                    let _ = input_sender.send(Input::Redraw).await;
                }

                _ => {}
            },
            _ => {}
        }
    }
}

pub async fn map_input_to_message(
    input_receiver: &mut Receiver<Input>,
    app_model: &app::Model,
) -> Option<app::Message> {
    match input_receiver.recv().await? {
        Input::Redraw => None,
        Input::KeyPress(key_event) => map_key_event_to_message(key_event, app_model),
        Input::RoomListUpdate(diffs) => {
            Some(app::Message::RoomList(mode::room_list::Message::UpdateRoomList(diffs)))
        }
        Input::TimelineUpdate(diffs) => {
            Some(app::Message::Room(room::Message::Timeline(timeline::Message::Update(diffs))))
        }
    }
}

fn map_key_event_to_message(key_event: KeyEvent, app_model: &app::Model) -> Option<app::Message> {
    let mode = &app_model.mode;

    Some(match key_event.code {
        KeyCode::Esc => app::Message::Mode(app::Mode::None),

        code => match mode {
            app::Mode::None => match code {
                KeyCode::Char('q') => app::Message::Quit,
                KeyCode::Char(' ') => {
                    app::Message::Mode(app::Mode::Space(mode::space::Model::new(
                        app_model.client.clone(),
                        app_model.sync_service.clone(),
                        app_model.input_sender.clone(),
                    )))
                }
                KeyCode::Char('r') => app::Message::Mode(app::Mode::Room(mode::room::Model::new(
                    app_model.room.is_some(),
                ))),
                KeyCode::Char('i') => app::Message::Mode(app::Mode::Insert),
                KeyCode::Up => app::Message::Room(room::Message::Timeline(
                    timeline::Message::Scroll(timeline::Scroll::Up),
                )),
                KeyCode::Down => app::Message::Room(room::Message::Timeline(
                    timeline::Message::Scroll(timeline::Scroll::Down),
                )),
                _ => return None,
            },

            app::Mode::Insert => app::Message::Room(room::Message::UpdateMessage(key_event)),

            app::Mode::Space(_) => app::Message::Space(match code {
                KeyCode::Char('f') => mode::space::Message::OpenRoomList,
                KeyCode::Char('S') => mode::space::Message::StartSyncService,
                KeyCode::Char('s') => mode::space::Message::StopSyncService,
                KeyCode::Char('c') => mode::space::Message::EmptyEventCache,
                KeyCode::Char('l') => mode::space::Message::OpenLogger,
                _ => return None,
            }),

            app::Mode::RoomList(_) => app::Message::RoomList(match code {
                KeyCode::Up => mode::room_list::Message::MoveCursorUp,
                KeyCode::Down => mode::room_list::Message::MoveCursorDown,
                KeyCode::Enter => mode::room_list::Message::Select,
                _ => mode::room_list::Message::UpdateFilter(key_event),
            }),

            app::Mode::Room(_) => app::Message::Room(match code {
                KeyCode::Char('b') => room::Message::Timeline(timeline::Message::PaginateBackwards),
                KeyCode::Char('r') => {
                    room::Message::Timeline(timeline::Message::ToggleReactionOnLastMessage)
                }
                KeyCode::Char('s') => {
                    room::Message::Timeline(timeline::Message::Scroll(timeline::Scroll::Start))
                }
                KeyCode::Char('e') => {
                    room::Message::Timeline(timeline::Message::Scroll(timeline::Scroll::End))
                }
                KeyCode::Char('t') => {
                    room::Message::Timeline(timeline::Message::ShowDetails(timeline::Details::None))
                }
                KeyCode::Char('i') => room::Message::Timeline(timeline::Message::ShowDetails(
                    timeline::Details::EventId,
                )),
                KeyCode::Char('o') => room::Message::Timeline(timeline::Message::ShowDetails(
                    timeline::Details::Origin,
                )),
                KeyCode::Char('l') => room::Message::Timeline(timeline::Message::ShowDetails(
                    timeline::Details::LinkedChunk,
                )),
                KeyCode::Char('m') => room::Message::MarkAsRead,
                KeyCode::Char('c') => room::Message::EmptyEventCache,
                _ => return None,
            }),

            app::Mode::Logger(_) => app::Message::Logger(match code {
                KeyCode::Char('l') => mode::logger::Message::OpenCommandPanel,
                KeyCode::Up => mode::logger::Message::Scroll(mode::logger::Scroll::Up),
                KeyCode::Down => mode::logger::Message::Scroll(mode::logger::Scroll::Down),
                KeyCode::Right => mode::logger::Message::IncreaseShownLogLevel,
                KeyCode::Left => mode::logger::Message::DecreaseShownLogLevel,
                KeyCode::Char('s') => mode::logger::Message::FocusFilter,
                KeyCode::Char('f') => mode::logger::Message::ToggleFilters,
                _ => return None,
            }),
        },
    })
}
