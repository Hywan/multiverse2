use futures::{FutureExt as _, StreamExt as _};
use std::io;
use tokio::runtime::{self, Builder, Runtime};
use tokio::sync::mpsc::{Receiver, Sender, channel};
use tokio::task::JoinHandle;
use tokio::task::spawn;

use crossterm::event::{
    self, Event as CrossTermEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyEventState,
    KeyModifiers,
};
use ratatui::{DefaultTerminal, Frame, buffer::Buffer, layout::Rect, text::Line, widgets::Widget};

#[derive(Debug)]
pub struct App {
    exit: bool,
    event_sender: Sender<Event>,
    event_receiver: Receiver<Event>,
}

impl App {
    pub fn new() -> Self {
        let (event_sender, event_receiver) = channel(128);

        Self {
            exit: false,
            event_sender,
            event_receiver,
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let runtime = Builder::new_multi_thread()
            .thread_name("multiverse2")
            .enable_all()
            .build()?;

        let _inputs_task = runtime.spawn({
            let event_sender = self.event_sender.clone();

            async move {
                handle_inputs(event_sender).await.unwrap();
            }
        });

        runtime.block_on(async move {
            while !self.exit {
                terminal.draw(|frame| self.draw(frame))?;
                self.handle_events().await?;
            }

            Ok(())
        })
    }

    fn draw(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    async fn handle_events(&mut self) -> io::Result<()> {
        match self.event_receiver.recv().await.unwrap() {
            Event::KeyPress(key_event) => self.handle_key_event(key_event),
        };

        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') => self.exit(),
            _ => {}
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Line::from("Hello").render(area, buffer);
    }
}

#[derive(Debug)]
enum Event {
    KeyPress(KeyEvent),
}

async fn handle_inputs(sender: Sender<Event>) -> io::Result<()> {
    let mut event_reader = EventStream::new();

    loop {
        match event_reader.next().fuse().await {
            Some(Ok(CrossTermEvent::Key(key_event))) if key_event.kind == KeyEventKind::Press => {
                let _ = sender.send(Event::KeyPress(key_event)).await;
            }
            _ => {}
        }
    }
}

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let app_result = App::new().run(&mut terminal);

    ratatui::restore();

    app_result
}
