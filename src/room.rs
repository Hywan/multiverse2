use crossterm::event::{KeyCode, KeyEvent};
use matrix_sdk::ruma::{
    api::client::receipt::create_receipt::v3::ReceiptType,
    events::room::message::RoomMessageEventContent,
};
use matrix_sdk_ui::room_list_service;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style, Styled},
    text::Line,
    widgets::Widget,
};
use tokio::sync::mpsc::Sender;

use crate::{TextArea, app, input::Input, timeline};

pub enum Message {
    UpdateMessage(KeyEvent),
    SendMessage,
    Timeline(timeline::Message),
    MarkAsRead,
    EmptyEventCache,
}

pub struct Model {
    room: room_list_service::Room,
    timeline: timeline::Model,
    message_textarea: TextArea,
}

impl Model {
    pub async fn new(room: room_list_service::Room, input_sender: Sender<Input>) -> Self {
        let timeline = timeline::Model::new(&room, Some(input_sender)).await;

        Self { room, timeline, message_textarea: TextArea::new_with_border() }
    }

    pub async fn update(&mut self, message: Message) -> Option<app::Message> {
        match message {
            Message::UpdateMessage(key_event) => {
                if key_event.code == KeyCode::Enter {
                    return Some(app::Message::Room(Message::SendMessage));
                }

                self.message_textarea.handle_input(key_event);

                return None;
            }
            Message::SendMessage => {
                let message = self.message_textarea.input();

                self.message_textarea.clear();

                if message.len() > 0 {
                    self.timeline
                        .timeline
                        .send(RoomMessageEventContent::text_plain(message).into())
                        .await
                        .unwrap();
                }

                self.timeline.update(timeline::Message::Scroll(timeline::Scroll::End)).await;
            }
            Message::Timeline(timeline_message) => {
                self.timeline.update(timeline_message).await;
            }
            Message::MarkAsRead => {
                self.timeline.timeline.mark_as_read(ReceiptType::Read).await.unwrap();
            }
            Message::EmptyEventCache => {
                if let Ok((room_event_cache, _event_cache_drop_handle)) =
                    self.room.event_cache().await
                {
                    room_event_cache.clear().await.unwrap();
                }
            }
        }

        Some(app::Message::Mode(app::Mode::None))
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        let [title_area, timeline_area, input_area] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Percentage(100),
            Constraint::Min(3),
        ])
        .areas(area);
        let timeline_area = timeline_area.inner(Margin::new(1, 0));

        Line::from(
            self.room
                .cached_display_name()
                .unwrap_or_else(|| self.room.id().as_str().to_owned())
                .set_style(Style::new().add_modifier(Modifier::BOLD)),
        )
        .centered()
        .render(title_area, buffer);
        self.timeline.render(timeline_area, buffer);
        self.message_textarea.render(input_area, buffer);
    }
}
