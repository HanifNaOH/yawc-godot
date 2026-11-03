use godot::prelude::*;
use tokio::sync::mpsc;

mod transport;
use transport::{Command, Event, Worker};

struct YawcGodotExtension;

#[gdextension]
unsafe impl ExtensionLibrary for YawcGodotExtension {}

#[derive(GodotClass)]
#[class(base=Node)]
struct YawcTransport {
    base: Base<Node>,
    worker: Worker,
}

#[godot_api]
impl YawcTransport {
    #[signal]
    fn opened();

    #[signal]
    fn closed();

    #[signal]
    fn error(message: GString);

    #[signal]
    fn text_message(message: GString);

    #[signal]
    fn binary_message(message: PackedByteArray);

    #[func]
    fn connect_to_url(&self, url: GString) -> bool {
        self.worker.send(Command::Connect(url.to_string()))
    }

    #[func]
    fn send_text(&self, message: GString) -> bool {
        self.worker.send(Command::SendText(message.to_string()))
    }

    #[func]
    fn send_binary(&self, message: PackedByteArray) -> bool {
        self.worker.send(Command::SendBinary(message.to_vec()))
    }

    #[func]
    fn close(&self) -> bool {
        self.worker.send(Command::Close)
    }
}

#[godot_api]
impl INode for YawcTransport {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            worker: Worker::start(),
        }
    }

    fn enter_tree(&mut self) {
        self.worker.restart();
    }

    fn process(&mut self, _delta: f64) {
        for _ in 0..64 {
            let event = match self.worker.try_recv() {
                Ok(event) => event,
                Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected) => {
                    break;
                }
            };

            match event {
                Event::Opened => self.signals().opened().emit(),
                Event::Closed => self.signals().closed().emit(),
                Event::Error(message) => self.signals().error().emit(message.as_str()),
                Event::TextMessage(message) => self.signals().text_message().emit(message.as_str()),
                Event::BinaryMessage(message) => self
                    .signals()
                    .binary_message()
                    .emit(&PackedByteArray::from(message)),
            }
        }
    }

    fn exit_tree(&mut self) {
        self.worker.shutdown();
    }
}
