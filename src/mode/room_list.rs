use std::{ops::Deref, sync::Arc};

use as_variant::as_variant;
use chrono::{DateTime, Local};
use crossterm::event::KeyEvent;
use futures::{StreamExt, pin_mut};
use matrix_sdk_ui::{
    RoomListService,
    eyeball_im::{Vector, VectorDiff},
    room_list_service::{RoomListDynamicEntriesController, RoomListItem, filters},
    sync_service::SyncService,
    timeline::{LatestEventValue, LatestEventValueLocalState, RoomExt, TimelineDetails},
};
use ratatui::{
    layout::{Constraint, Flex, Layout, Margin, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, StatefulWidget, Widget,
    },
};
use tokio::{
    spawn,
    sync::{mpsc::Sender, oneshot},
};

use crate::{
    TextArea, app,
    block::{BORDER_STYLE, NO_PADDING, PADDING, block_with_title},
    input::Input,
    task_ext::{AbortOnDrop, JoinHandleExt},
    timeline::{self, render_timeline_item_content},
};

#[derive(Debug)]
pub enum Message {
    UpdateFilter(KeyEvent),
    UpdateRoomList(Vec<VectorDiff<RoomListItem>>),
    MoveCursorUp,
    MoveCursorDown,
    Select,
}

pub struct Model {
    room_list_controller: RoomListDynamicEntriesController,
    _room_list_updates_handle: AbortOnDrop<()>,
    rooms: Vector<(RoomListItem, Arc<LatestEventValue>)>,
    list_state: ListState,
    search_textarea: TextArea,
    selected_room_timeline: Option<timeline::Model>,
}

impl Model {
    pub async fn new(sync_service: Arc<SyncService>, input_sender: Sender<Input>) -> Self {
        let room_list_service = sync_service.room_list_service();

        let (room_list_controller_sender, room_list_controller_receiver) = oneshot::channel();

        let _room_list_updates_handle = spawn(room_list_updates_task(
            room_list_service.clone(),
            room_list_controller_sender,
            input_sender,
        ))
        .abort_on_drop();

        let room_list_controller = room_list_controller_receiver.await.unwrap();

        room_list_controller.set_filter(Box::new(filters::new_filter_non_left()));

        Self {
            room_list_controller,
            _room_list_updates_handle,
            rooms: Vector::new(),
            list_state: ListState::default(),
            search_textarea: TextArea::new(),
            selected_room_timeline: None,
        }
    }
}

impl Model {
    pub async fn update(&mut self, message: Message) -> Option<app::Message> {
        Some(match message {
            Message::UpdateFilter(key_event) => {
                if self.search_textarea.handle_input(key_event) {
                    let search_term = self.search_textarea.input();

                    if search_term.len() == 0 {
                        self.room_list_controller
                            .set_filter(Box::new(filters::new_filter_non_left()));
                    } else {
                        self.room_list_controller.set_filter(Box::new(filters::new_filter_all(
                            vec![
                                Box::new(filters::new_filter_non_left()),
                                Box::new(filters::new_filter_fuzzy_match_room_name(&search_term)),
                            ],
                        )));
                    }
                }

                return None;
            }
            Message::UpdateRoomList(diffs) => {
                for diff in diffs {
                    async fn map(room: RoomListItem) -> (RoomListItem, Arc<LatestEventValue>) {
                        let latest_event_value = Arc::new(room.latest_event().await);

                        (room, latest_event_value)
                    }

                    let diff = match diff {
                        VectorDiff::Append { values } => VectorDiff::Append {
                            values: {
                                let mut new_values = Vector::new();

                                for value in values {
                                    new_values.push_back(map(value).await);
                                }

                                new_values
                            },
                        },
                        VectorDiff::Clear => VectorDiff::Clear,
                        VectorDiff::PushFront { value } => {
                            VectorDiff::PushFront { value: map(value).await }
                        }
                        VectorDiff::PushBack { value } => {
                            VectorDiff::PushBack { value: map(value).await }
                        }
                        VectorDiff::PopFront => VectorDiff::PopFront,
                        VectorDiff::PopBack => VectorDiff::PopBack,
                        VectorDiff::Insert { index, value } => {
                            VectorDiff::Insert { index, value: map(value).await }
                        }
                        VectorDiff::Set { index, value } => {
                            VectorDiff::Set { index, value: map(value).await }
                        }
                        VectorDiff::Remove { index } => VectorDiff::Remove { index },
                        VectorDiff::Truncate { length } => VectorDiff::Truncate { length },
                        VectorDiff::Reset { values } => VectorDiff::Reset {
                            values: {
                                let mut new_values = Vector::new();

                                for value in values {
                                    new_values.push_back(map(value).await);
                                }

                                new_values
                            },
                        },
                    };

                    diff.apply(&mut self.rooms);
                }

                if self.list_state.selected().is_none() {
                    self.list_state.select_first();
                }

                self.update_selected_room_timeline().await;

                return None;
            }
            Message::MoveCursorUp => {
                self.list_state.select_previous();
                self.update_selected_room_timeline().await;
                return None;
            }
            Message::MoveCursorDown => {
                self.list_state.select_next();
                self.update_selected_room_timeline().await;
                return None;
            }
            Message::Select => {
                let Some((room, _)) = self.rooms.get(self.list_state.selected().unwrap_or(0))
                else {
                    return None;
                };

                app::Message::OpenRoom(room.deref().clone())
            }
        })
    }

    pub async fn update_selected_room_timeline(&mut self) {
        self.selected_room_timeline = match self.rooms.get(self.list_state.selected().unwrap_or(0))
        {
            Some((room, _)) => Some(timeline::Model::new(room, None).await),
            None => None,
        };
    }

    pub fn render(&mut self, area: Rect, buffer: &mut ratatui::buffer::Buffer) {
        let [area] =
            Layout::horizontal([Constraint::Percentage(90)]).flex(Flex::Center).areas(area);
        let [area] = Layout::vertical([Constraint::Percentage(80)]).flex(Flex::Center).areas(area);

        Clear.render(area, buffer);

        let (list_area, preview_area) = if area.width < 80 {
            (area, None)
        } else {
            let [left, right] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(area);

            (left, Some(right))
        };

        let list_block = block_with_title("Room list").padding(NO_PADDING);

        let [input_area, table_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Percentage(100)])
                .areas(list_block.inner(list_area));

        list_block.render(list_area, buffer);

        self.search_textarea.render(input_area.inner(Margin::new(1, 0)), buffer);
        const HIGHLIGHT_SYMBOL: &str = " > ";
        StatefulWidget::render(
            List::new(self.rooms.iter().map(|(room, latest_event)| {
                ListItem::new({
                    let mut output = Text::default();

                    output.push_line(Line::default().spans({
                        let room_name = room
                            .cached_display_name()
                            .map(|display_name| display_name.to_string())
                            .unwrap_or_else(|| room.room_id().as_str().to_owned());

                        let spaces = str::repeat(
                            " ",
                            usize::from(table_area.width)
                                .saturating_sub(room_name.len())
                                .saturating_sub(HIGHLIGHT_SYMBOL.len())
                                .saturating_sub(
                                    1 /* borders */
                                        + usize::from(PADDING.left)
                                        + usize::from(PADDING.right),
                                )
                                .saturating_sub(5),
                        );

                        let time = match latest_event.deref() {
                            LatestEventValue::None => Span::raw("???"),
                            LatestEventValue::Remote { timestamp, .. }
                            | LatestEventValue::Local { timestamp, .. } => Span::raw(
                                DateTime::<Local>::from(
                                    timestamp.to_system_time().expect("invalid system time"),
                                )
                                .format("%H:%M")
                                .to_string(),
                            ),
                        };

                        [room_name.bold(), spaces.into(), time]
                    }));

                    output.push_line(Line::default().spans({
                        let (sender, decoration, content) = match latest_event.deref() {
                            LatestEventValue::None => {
                                (Span::raw(""), "".into(), Span::raw("<none>"))
                            }
                            LatestEventValue::Remote { sender, profile, content, .. } => {
                                let mut sender = as_variant!(profile, TimelineDetails::Ready(profile) => profile)
                                    .and_then(|profile| profile.display_name.clone())
                                    .unwrap_or_else(|| sender.localpart().to_owned());
                                sender.push_str(": ");

                                let content = render_timeline_item_content(content, &area).swap_remove(0);

                                (sender.into(), "".into(), content)
                            }
                            LatestEventValue::Local { state, content, .. } => {
                                let content = render_timeline_item_content(content, &area).swap_remove(0);

                                (
                                    Span::raw("Me: "),
                                    Span::raw(match state {
                                        LatestEventValueLocalState::IsSending => "🕙 ",
                                        LatestEventValueLocalState::CannotBeSent => "❗️ ",
                                        LatestEventValueLocalState::HasBeenSent => "",
                                    }),
                                    content.into(),
                                )
                            }
                        };

                        [decoration, sender, content.italic()]
                    }));

                    output
                })
            }))
            .highlight_style(Style::new().bg(Color::DarkGray))
            .highlight_symbol(" > ")
            .block(Block::new().borders(Borders::TOP).border_style(BORDER_STYLE).padding(PADDING)),
            table_area,
            buffer,
            &mut self.list_state,
        );

        if let Some(preview_area) = preview_area {
            Paragraph::new("").block(block_with_title("Room preview")).render(preview_area, buffer);

            let preview_area = preview_area.inner(Margin { horizontal: 2, vertical: 1 });
            Clear.render(preview_area, buffer);

            if let Some(timeline) = &self.selected_room_timeline {
                timeline.render(preview_area, buffer);
            }
        }
    }
}

async fn room_list_updates_task(
    room_list_service: Arc<RoomListService>,
    room_list_controller_sender: oneshot::Sender<RoomListDynamicEntriesController>,
    input_sender: Sender<Input>,
) {
    let all_rooms = room_list_service.all_rooms().await.unwrap();
    let (rooms_stream, room_list_controller) =
        all_rooms.entries_with_dynamic_adapters(u16::MAX as usize);

    let _ = room_list_controller_sender.send(room_list_controller);

    pin_mut!(rooms_stream);

    while let Some(diffs) = rooms_stream.next().await {
        let _ = input_sender.send(Input::RoomListUpdate(diffs)).await;
    }
}
