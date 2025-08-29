use std::{borrow::Cow, cmp::min, iter, ops::Not, sync::Arc};

use chrono::{DateTime, Local};
use futures::{StreamExt, pin_mut};
use itertools::Itertools as _;
use matrix_sdk::{
    Client, Room,
    deserialized_responses::TimelineEvent,
    linked_chunk::{ChunkContent, ChunkIdentifier, LinkedChunkId},
    locks::Mutex,
    ruma::{EventId, OwnedEventId, OwnedRoomId},
};
use matrix_sdk_ui::{
    Timeline,
    eyeball_im::{Vector, VectorDiff},
    timeline::{
        MsgLikeKind, Profile, RoomExt, TimelineDetails, TimelineItem, TimelineItemContent,
        TimelineItemKind, VirtualTimelineItem,
    },
};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Margin, Rect},
    style::{Color, Style, Styled, Stylize},
    text::{Line, Span, Text},
    widgets::{
        List, ListDirection, ListItem, Paragraph, ScrollbarOrientation, ScrollbarState,
        StatefulWidget, Widget,
    },
};
use tokio::{spawn, sync::mpsc::Sender};

use crate::{
    app, block,
    input::Input,
    scrollbar,
    task_ext::{AbortOnDrop, JoinHandleExt},
};

pub enum Scroll {
    Up,
    Down,
    Start,
    End,
}

#[derive(Default)]
pub enum Details {
    #[default]
    None,
    EventId,
    Origin,
    LinkedChunk,
}

pub enum Message {
    Update(Vec<VectorDiff<Arc<TimelineItem>>>),
    Scroll(Scroll),
    PaginateBackwards,
    ShowDetails(Details),
    ToggleReactionOnLastMessage,
}

const MINIMUM_NUMBER_OF_VISIBLE_ITEMS: usize = 3;

pub struct Model {
    pub(crate) timeline: Arc<Timeline>,
    client: Client,
    room_id: OwnedRoomId,
    items: Vector<Arc<TimelineItem>>,
    linked_chunks: Vec<(ChunkIdentifier, ChunkContent<TimelineEvent, String>)>,
    _items_updates_handle: Option<AbortOnDrop<()>>,
    scroll_position: Mutex<usize>,
    details: Details,
}

impl Model {
    pub async fn new(room: &Room, input_sender: Option<Sender<Input>>) -> Self {
        let timeline = Arc::new(room.timeline_builder().build().await.unwrap());
        let client = room.client();
        let room_id = timeline.room().room_id().to_owned();
        let mut items = Vector::new();

        let _items_updates_handle = match input_sender {
            // Run the task to update the timeline items.
            Some(input_sender) => {
                Some(spawn(items_updates_task(timeline.clone(), input_sender)).abort_on_drop())
            }
            // Initialise the timeline items without listening to the stream of updates.
            None => {
                let (initial_items, _) = timeline.subscribe().await;

                VectorDiff::Reset { values: initial_items }.apply(&mut items);

                None
            }
        };

        Self {
            timeline,
            client,
            room_id,
            items,
            linked_chunks: Vec::new(),
            _items_updates_handle,
            scroll_position: Mutex::new(0),
            details: Details::default(),
        }
    }

    pub async fn update(&mut self, message: Message) -> Option<app::Message> {
        match message {
            Message::Update(diffs) => {
                let mut recompute_linked_chunks = false;

                for diff in diffs {
                    // If the diff is not `VectorDiff::Set`, we need to
                    // recompute the linked chunks.
                    if recompute_linked_chunks.not() && matches!(diff, VectorDiff::Set { .. }).not()
                    {
                        recompute_linked_chunks = true;
                    }

                    diff.apply(&mut self.items);
                }

                if recompute_linked_chunks.not() {
                    return None;
                }

                reload_linked_chunks(
                    &mut self.linked_chunks,
                    &self.client,
                    &self.room_id,
                    self.items.iter().find_map(|item| item.as_event()?.event_id()),
                )
                .await?;
            }
            Message::Scroll(scroll) => {
                let mut scroll_position = self.scroll_position.lock();

                *scroll_position = match &self.details {
                    Details::None | Details::EventId | Details::Origin => {
                        update_scroll_position_for_timeline(&scroll, *scroll_position, &self.items)
                    }
                    Details::LinkedChunk => {
                        update_scroll_position_for_linked_chunk(&scroll, *scroll_position)
                    }
                };
            }
            Message::PaginateBackwards => {
                // TODO: do something with the result.
                let _ = self.timeline.paginate_backwards(20).await;
            }
            Message::ShowDetails(details) => {
                if matches!(
                    (&self.details, &details),
                    (Details::None | Details::EventId, Details::LinkedChunk)
                        | (Details::LinkedChunk, Details::None | Details::EventId)
                ) {
                    *self.scroll_position.lock() = 0;
                }

                self.details = details;
            }
            Message::ToggleReactionOnLastMessage => {
                if let Some(last_timeline_item_id) =
                    self.items.iter().rev().find_map(|timeline_item| {
                        timeline_item.as_event().and_then(|event_timeline_item| {
                            event_timeline_item
                                .content()
                                .as_message()
                                .is_some()
                                .then(|| event_timeline_item.identifier())
                        })
                    })
                {
                    self.timeline.toggle_reaction(&last_timeline_item_id, "👍").await.unwrap();
                }
            }
        }

        None
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        if let Details::LinkedChunk = &self.details {
            self.render_linked_chunk(area, buffer);
        } else {
            self.render_timeline(area, buffer);
        }
    }

    pub fn render_linked_chunk(&self, area: Rect, buffer: &mut Buffer) {
        let mut text = Text::raw("");
        const INLINE_MARGIN: usize = 3;
        let block_width = (area.width as usize).saturating_sub(INLINE_MARGIN * 2);
        let block_text_width = block_width.saturating_sub(4);

        let scrollbar_area = area;
        let area = scrollbar_area.inner(Margin { horizontal: 2, vertical: 0 });

        for (chunk_identifier, chunk_content) in self.linked_chunks.iter().rev() {
            match chunk_content {
                ChunkContent::Items(events) => {
                    let event_ids = textwrap::wrap(
                        &events
                            .iter()
                            .map(|event| {
                                event
                                    .event_id()
                                    .map(|event_id| format_event_id(event_id))
                                    .unwrap_or_else(|| "???".to_owned())
                            })
                            .join(", "),
                        textwrap::Options::new(block_text_width).break_words(false),
                    )
                    .into_iter()
                    .map(Cow::into_owned)
                    .collect::<Vec<_>>();

                    let border_set = block::BORDER_TYPE.to_border_set();

                    // Top border.
                    text.push_line(Line::from(format!(
                        "{}{}{}",
                        border_set.top_left,
                        iter::repeat_n(border_set.horizontal_top, block_width.saturating_sub(2))
                            .join(""),
                        border_set.top_right
                    )));
                    // Header.
                    text.push_line(Line::from(format!(
                        "{border_left} Chunk identifier #{chunk_identifier:<fill$} {border_right}",
                        chunk_identifier = chunk_identifier.index(),
                        fill = block_width.saturating_sub(4).saturating_sub(18),
                        border_left = border_set.vertical_left,
                        border_right = border_set.vertical_right,
                    )));
                    // Empty line.
                    text.push_line(format!(
                        "{border_left}{border_right:>fill$}",
                        fill = block_width.saturating_sub(1),
                        border_left = border_set.vertical_left,
                        border_right = border_set.vertical_right
                    ));
                    // Event IDs.
                    text.extend(event_ids.into_iter().map(|event_ids| {
                        Line::from(format!(
                            "{border_left} {line:<fill$} {border_right}",
                            line = event_ids,
                            fill = block_width.saturating_sub(4),
                            border_left = border_set.vertical_left,
                            border_right = border_set.vertical_right,
                        ))
                    }));
                    // Bottom border.
                    text.push_line(Line::from(format!(
                        "{}{}{}",
                        border_set.bottom_left,
                        iter::repeat_n(border_set.horizontal_bottom, block_width.saturating_sub(2))
                            .join(""),
                        border_set.bottom_right
                    )));
                }
                ChunkContent::Gap(prev_token) => {
                    text.push_line(
                        Line::from(format!("Gap {prev_token}")).alignment(Alignment::Center),
                    );
                }
            }

            text.push_line(Span::raw("↑").into_centered_line());
        }

        let text_height = text.height();
        let paragraph = Paragraph::new(text);
        let area_height = area.height as usize;

        if area_height > text_height {
            paragraph
        } else {
            let scroll_length = text_height - area_height;

            let mut scroll_position = self.scroll_position.lock();
            *scroll_position = min(*scroll_position, scroll_length);
            let scroll_position = *scroll_position;

            let mut state =
                ScrollbarState::new(scroll_length).position(scroll_length - scroll_position);

            StatefulWidget::render(
                scrollbar::scrollbar(ScrollbarOrientation::VerticalRight),
                scrollbar_area,
                buffer,
                &mut state,
            );

            paragraph.scroll(((scroll_length - scroll_position) as u16, 0))
        }
        .render(area, buffer);
    }

    pub fn render_timeline(&self, area: Rect, buffer: &mut Buffer) {
        let mut list_total_height: usize = 0;
        let mut list_skipped_height: usize = 0;
        let scroll_position = *self.scroll_position.lock();
        let list = List::new(
            self.items
                .iter()
                .rev()
                .enumerate()
                .filter_map(|(nth, item)| Some((nth, self.render_timeline_item(item, &area)?)))
                .inspect(|(_nth, item)| {
                    list_total_height += item.height();
                })
                .skip_while(|(nth, item)| {
                    list_skipped_height += item.height();

                    *nth < scroll_position
                })
                .map(|(_, item)| item),
        )
        .direction(ListDirection::BottomToTop);

        let mut state = ScrollbarState::new(list_total_height)
            .position(list_total_height - list_skipped_height + 1);

        StatefulWidget::render(
            scrollbar::scrollbar(ScrollbarOrientation::VerticalRight),
            area,
            buffer,
            &mut state,
        );

        let area = area.inner(Margin { horizontal: 2, vertical: 0 });

        Widget::render(list, area, buffer);
    }

    pub fn render_timeline_item<'a>(
        &self,
        item: &'a Arc<TimelineItem>,
        area: &'a Rect,
    ) -> Option<ListItem<'a>> {
        Some(ListItem::new(match item.kind() {
            TimelineItemKind::Event(event_item) => {
                let content = event_item.content();
                let mut output = Text::default();

                // Sender and time.
                {
                    let sender = if let TimelineDetails::Ready(Profile {
                        display_name: Some(display_name),
                        ..
                    }) = event_item.sender_profile()
                    {
                        Span::raw(display_name)
                    } else {
                        Span::raw(event_item.sender().as_str())
                    };

                    let time = if let Some(time) = event_item.timestamp().to_system_time() {
                        Span::raw(DateTime::<Local>::from(time).format("%H:%M").to_string())
                    } else {
                        Span::raw("???")
                    };

                    output.push_line(Line::default().spans([
                        sender.yellow(),
                        " ".into(),
                        time.dark_gray(),
                    ]));
                }

                // Message.
                {
                    let mut spans = vec![];

                    if matches!(&self.details, Details::EventId | Details::Origin) {
                        let id = event_item
                            .event_id()
                            .map(|event_id| event_id.as_str())
                            .or_else(|| {
                                event_item
                                    .transaction_id()
                                    .map(|transaction_id| transaction_id.as_str())
                            })
                            .unwrap_or_else(|| "no ID");

                        if let Details::Origin = &self.details {
                            spans.push(Span::raw(format!("{id} is from")));
                            match event_item.origin() {
                                Some(origin) => {
                                    spans.push(Span::styled(
                                        format!("{origin:?}"),
                                        Style::default().green().bold(),
                                    ));
                                }
                                None => spans
                                    .push(Span::styled("unknown", Style::default().red().bold())),
                            }
                        } else {
                            spans.push(Span::styled(id, Style::default().green().bold()));
                        }
                    } else {
                        let non_message_style = Style::default().fg(Color::Indexed(247)).italic();

                        match content {
                            TimelineItemContent::MsgLike(message_like) => {
                                match &message_like.kind {
                                    MsgLikeKind::Message(message) => {
                                        spans.extend(
                                            textwrap::wrap(message.body(), area.width as usize - 2)
                                                .into_iter()
                                                .map(Span::raw),
                                        );
                                    }
                                    MsgLikeKind::UnableToDecrypt(_) => {
                                        spans.push(Span::styled(
                                            "<unable to decrypt>",
                                            non_message_style.fg(Color::Red),
                                        ));
                                    }
                                    MsgLikeKind::Redacted => {
                                        spans.push(Span::styled("<redacted>", non_message_style));
                                    }
                                    MsgLikeKind::Poll(_) => {
                                        spans.push(Span::styled("<poll>", non_message_style));
                                    }
                                    _ => {
                                        spans.push(Span::styled(
                                            "<unsupported messagge-like event>",
                                            non_message_style,
                                        ));
                                    }
                                }
                            }
                            TimelineItemContent::MembershipChange(membership_change) => {
                                spans.push(Span::styled(
                                    format!(
                                        "<membership change `{:?}`>",
                                        membership_change.change()
                                    ),
                                    non_message_style,
                                ));
                            }
                            TimelineItemContent::ProfileChange(_) => {
                                spans.push(Span::styled("<profile change>", non_message_style));
                            }
                            TimelineItemContent::OtherState(other_state) => {
                                spans.push(Span::styled(
                                    format!("<state `{}`>", other_state.content().event_type()),
                                    non_message_style,
                                ));
                            }
                            TimelineItemContent::CallInvite => {
                                spans.push(Span::styled("<call invite>", non_message_style));
                            }
                            TimelineItemContent::CallNotify => {
                                spans.push(Span::styled("<call notify>", non_message_style));
                            }
                            TimelineItemContent::FailedToParseMessageLike { .. } => {
                                spans.push(Span::styled(
                                    "<failed to parse message-like>",
                                    non_message_style.fg(Color::Red),
                                ));
                            }
                            TimelineItemContent::FailedToParseState { .. } => {
                                spans.push(Span::styled(
                                    "<failed to parse state>",
                                    non_message_style.fg(Color::Red),
                                ));
                            }
                        }
                    }

                    let is_local_item = event_item.is_local_echo();

                    output.extend(spans.into_iter().map(|span| {
                        if is_local_item {
                            span.set_style(Style::default().italic().dim())
                        } else {
                            span
                        }
                    }));
                }

                // Reactions.
                if matches!(self.details, Details::None) {
                    let reactions = content.reactions();

                    if let Some(reactions) = reactions {
                        let mut line = Line::raw("");
                        let style = Style::default().bg(Color::Rgb(71, 79, 102));

                        line.extend(reactions.iter().map(|(reaction, senders)| {
                            let number_of_senders = senders.len();

                            Span::styled(
                                if number_of_senders > 1 {
                                    format!(" {}×{}", reaction, number_of_senders)
                                } else {
                                    format!(" {}", reaction)
                                },
                                style,
                            )
                        }));
                        line.push_span(Span::styled(" ", style));

                        output.push_line(line);
                    }
                }

                // Read receipts.
                if matches!(self.details, Details::None) {
                    let read_receipts = event_item.read_receipts();

                    if read_receipts.is_empty().not() {
                        let style = Style::default().dark_gray().italic();

                        let mut line = Line::styled("read by ", style);
                        let mut read_receipts = read_receipts.iter().peekable();

                        while let Some((user_id, _)) = read_receipts.next() {
                            line.push_span(Span::raw(user_id.localpart()));

                            if read_receipts.peek().is_some() {
                                line.push_span(", ");
                            }
                        }

                        output.push_line(line);
                    }
                }

                output.push_line("\n");

                // Right align event sent by us.
                {
                    if event_item.is_own() {
                        output = output.right_aligned();
                    }
                }

                output
            }

            TimelineItemKind::Virtual(virtual_item) => match virtual_item {
                VirtualTimelineItem::DateDivider(time) => {
                    let time = if let Some(time) = time.to_system_time() {
                        Span::raw(DateTime::<Local>::from(time).format("%a, %e %b %Y").to_string())
                    } else {
                        Span::raw("date divider")
                    };

                    let mut text = Text::default().centered();
                    text.push_span("───── ");
                    text.push_span(time);
                    text.push_span(" ─────");

                    text
                }
                VirtualTimelineItem::ReadMarker => return None,
                VirtualTimelineItem::TimelineStart => {
                    let mut text = Text::default().centered();

                    text.push_span("───── ");
                    text.push_span("Beginning of the room");
                    text.push_span(" ─────");

                    text
                }
            },
        }))
    }
}

// Load all chunks until one doesn't contain the first timeline item's event.
async fn reload_linked_chunks(
    linked_chunks: &mut Vec<(ChunkIdentifier, ChunkContent<TimelineEvent, String>)>,
    client: &Client,
    room_id: &OwnedRoomId,
    first_event_id: Option<&EventId>,
) -> Option<()> {
    linked_chunks.clear();

    let event_cache_store = client.event_cache_store();
    let event_cache_store = event_cache_store.lock().await.unwrap();

    let (mut next_chunk, _) =
        event_cache_store.load_last_chunk(LinkedChunkId::Room(room_id)).await.unwrap();

    let Some(first_event_id) = first_event_id else {
        return None;
    };

    while let Some(chunk) = next_chunk {
        match chunk.content {
            ChunkContent::Items(events) => {
                let mut do_break = false;

                if events
                    .iter()
                    .filter_map(|event| event.event_id())
                    .any(|event_id| &event_id == first_event_id)
                {
                    do_break = true;
                }

                linked_chunks.push((chunk.identifier, ChunkContent::Items(events)));

                if do_break {
                    break;
                }
            }
            ChunkContent::Gap(gap) => {
                linked_chunks.push((chunk.identifier, ChunkContent::Gap(gap.prev_token)));
            }
        }

        next_chunk = event_cache_store
            .load_previous_chunk(LinkedChunkId::Room(room_id), chunk.identifier)
            .await
            .unwrap();
    }

    Some(())
}

async fn items_updates_task(timeline: Arc<Timeline>, input_sender: Sender<Input>) {
    let (initial_items, items_stream) = timeline.subscribe().await;

    let _ = input_sender
        .send(Input::TimelineUpdate(vec![VectorDiff::Reset { values: initial_items }]))
        .await;

    pin_mut!(items_stream);

    while let Some(diffs) = items_stream.next().await {
        let _ = input_sender.send(Input::TimelineUpdate(diffs)).await;
    }
}

fn update_scroll_position_for_timeline(
    scroll: &Scroll,
    scroll_position: usize,
    items: &Vector<Arc<TimelineItem>>,
) -> usize {
    match scroll {
        Scroll::Up => min(
            items.len().saturating_sub(MINIMUM_NUMBER_OF_VISIBLE_ITEMS),
            scroll_position.saturating_add(1),
        ),
        Scroll::Down => min(
            items.len().saturating_sub(MINIMUM_NUMBER_OF_VISIBLE_ITEMS),
            scroll_position.saturating_sub(1),
        ),
        Scroll::Start => items.len().saturating_sub(MINIMUM_NUMBER_OF_VISIBLE_ITEMS),
        Scroll::End => 0,
    }
}

fn update_scroll_position_for_linked_chunk(scroll: &Scroll, scroll_position: usize) -> usize {
    match scroll {
        Scroll::Up => scroll_position.saturating_add(1),
        Scroll::Down => scroll_position.saturating_sub(1),
        Scroll::Start => {
            // We don't know the size of the linked chunk rendering. Let's set
            // to `usize::MAX` and let `render` updates the scroll position. Not
            // ideal, but eh, it's simpler like this.
            usize::MAX
        }
        Scroll::End => 0,
    }
}

fn format_event_id(event_id: OwnedEventId) -> String {
    let event_id = event_id.as_str();

    if event_id.len() > 8 {
        format!("{start}~{end}", start = &event_id[..5], end = &event_id[event_id.len() - 3..])
    } else {
        event_id.to_owned()
    }
}
