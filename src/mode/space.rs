use std::sync::Arc;

use matrix_sdk::Client;
use matrix_sdk_ui::sync_service::SyncService;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    widgets::{Cell, Clear, Row, Table, Widget},
};
use tokio::sync::mpsc::Sender;

use crate::{app, block::block_with_title, input::Input, mode};

#[derive(Debug)]
pub enum Message {
    OpenRoomList,
    StartSyncService,
    StopSyncService,
    EmptyEventCache,
    OpenLogger,
}

pub struct Model {
    client: Client,
    sync_service: Arc<SyncService>,
    input_sender: Sender<Input>,
}

impl Model {
    pub fn new(
        client: Client,
        sync_service: Arc<SyncService>,
        input_sender: Sender<Input>,
    ) -> Self {
        Self { client, sync_service, input_sender }
    }

    pub async fn update(&mut self, message: Message) -> Option<app::Message> {
        Some(match message {
            Message::OpenRoomList => app::Message::Mode(app::Mode::RoomList(
                mode::room_list::Model::new(self.sync_service.clone(), self.input_sender.clone())
                    .await,
            )),
            Message::StartSyncService => {
                self.sync_service.start().await;
                app::Message::Mode(app::Mode::None)
            }
            Message::StopSyncService => {
                self.sync_service.stop().await;
                app::Message::Mode(app::Mode::None)
            }
            Message::EmptyEventCache => {
                self.client
                    .event_cache_store()
                    .lock()
                    .await
                    .unwrap()
                    .clear_all_rooms_chunks()
                    .await
                    .unwrap();
                app::Message::Mode(app::Mode::None)
            }
            Message::OpenLogger => app::Message::Mode(app::Mode::Logger(mode::logger::Model::new(
                self.input_sender.clone(),
            ))),
        })
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        let rows = [
            Row::new([Cell::new("f"), Cell::new("Open room list")]),
            Row::new([Cell::new("S"), Cell::new("Start the sync service")]),
            Row::new([Cell::new("s"), Cell::new("Stop the sync service")]),
            Row::new([Cell::new("c"), Cell::new("Empty all room event caches")]),
            Row::new([Cell::new("l"), Cell::new("Open logger")]),
        ];

        let [_, area] =
            Layout::vertical([Constraint::Percentage(100), Constraint::Min(rows.len() as u16 + 2)])
                .areas(area);
        let [_, area] =
            Layout::horizontal([Constraint::Percentage(100), Constraint::Min(35)]).areas(area);

        Clear.render(area, buffer);

        Table::default()
            .rows(rows)
            .widths([Constraint::Length(3), Constraint::Percentage(100)])
            .block(block_with_title("Space"))
            .render(area, buffer);
    }
}
