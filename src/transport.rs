use std::{
    thread::{self, JoinHandle},
    time::Duration,
};

use futures_util::SinkExt;
use tokio::sync::{mpsc, watch};
use yawc::{Frame, Options, WebSocket, close::CloseCode, frame::OpCode};

const COMMAND_CAPACITY: usize = 64;
const EVENT_CAPACITY: usize = 256;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) enum Command {
    Connect(String),
    SendText(String),
    Close,
}

pub(crate) enum Event {
    Opened,
    Closed,
    Error(String),
    TextMessage(String),
}

pub(crate) struct Worker {
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<Event>,
    shutdown: watch::Sender<bool>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub(crate) fn start() -> Self {
        let (command_tx, command_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (event_tx, event_rx) = mpsc::channel(EVENT_CAPACITY);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let thread = thread::Builder::new()
            .name("yawc-transport".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();

                match runtime {
                    Ok(runtime) => runtime.block_on(run_worker(command_rx, event_tx, shutdown_rx)),
                    Err(error) => {
                        let _ = event_tx.blocking_send(Event::Error(error.to_string()));
                    }
                }
            })
            .expect("failed to start Yawc transport worker");

        Self {
            commands: command_tx,
            events: event_rx,
            shutdown: shutdown_tx,
            thread: Some(thread),
        }
    }

    pub(crate) fn send(&self, command: Command) -> bool {
        self.commands.try_send(command).is_ok()
    }

    pub(crate) fn try_recv(&mut self) -> Result<Event, mpsc::error::TryRecvError> {
        self.events.try_recv()
    }

    pub(crate) fn shutdown(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

async fn run_worker(
    mut commands: mpsc::Receiver<Command>,
    events: mpsc::Sender<Event>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut pending_command = None;

    'worker: loop {
        let command = match pending_command.take() {
            Some(command) => command,
            None => {
                tokio::select! {
                    _ = shutdown.changed() => return,
                    command = commands.recv() => match command {
                        Some(command) => command,
                        None => return,
                    },
                }
            }
        };

        match command {
            Command::Connect(url) => {
                let parsed_url = match url::Url::parse(&url) {
                    Ok(url) => url,
                    Err(error) => {
                        if !report_failure(&events, &mut shutdown, error.to_string()).await {
                            return;
                        }
                        continue 'worker;
                    }
                };
                let connection = tokio::time::timeout(
                    CONNECTION_TIMEOUT,
                    WebSocket::connect(parsed_url)
                        .with_options(Options::default().with_balanced_compression()),
                );
                tokio::pin!(connection);

                let mut connected_socket = None;
                'connecting: loop {
                    tokio::select! {
                        _ = shutdown.changed() => return,
                        result = &mut connection => {
                            match result {
                                Ok(Ok(socket)) => connected_socket = Some(socket),
                                Ok(Err(error)) => {
                                    if !report_failure(&events, &mut shutdown, error.to_string()).await {
                                        return;
                                    }
                                }
                                Err(_) => {
                                    let message = format!(
                                        "WebSocket connection timed out after {} seconds",
                                        CONNECTION_TIMEOUT.as_secs(),
                                    );
                                    if !report_failure(&events, &mut shutdown, message).await {
                                        return;
                                    }
                                }
                            }
                            break 'connecting;
                        }
                        command = commands.recv() => match command {
                            Some(Command::Close) => {
                                if !publish(&events, &mut shutdown, Event::Closed).await {
                                    return;
                                }
                                break 'connecting;
                            }
                            Some(Command::Connect(next_url)) => {
                                pending_command = Some(Command::Connect(next_url));
                                break 'connecting;
                            }
                            Some(Command::SendText(_)) => {
                                if !publish(
                                    &events,
                                    &mut shutdown,
                                    Event::Error("cannot send text before the connection opens".to_owned()),
                                ).await {
                                    return;
                                }
                            }
                            None => return,
                        }
                    }
                }

                let Some(mut socket) = connected_socket else {
                    continue 'worker;
                };
                if !publish(&events, &mut shutdown, Event::Opened).await {
                    return;
                }

                'connected: loop {
                    tokio::select! {
                        _ = shutdown.changed() => return,
                        frame = socket.next_frame() => match frame {
                            Ok(frame) if frame.opcode() == OpCode::Text => {
                                if !publish(
                                    &events,
                                    &mut shutdown,
                                    Event::TextMessage(frame.as_str().to_owned()),
                                ).await {
                                    return;
                                }
                            }
                            Ok(frame) if frame.opcode() == OpCode::Close => {
                                if !publish(&events, &mut shutdown, Event::Closed).await {
                                    return;
                                }
                                break 'connected;
                            }
                            Ok(_) => {}
                            Err(error) => {
                                if !report_failure(&events, &mut shutdown, error.to_string()).await {
                                    return;
                                }
                                break 'connected;
                            }
                        },
                        command = commands.recv() => match command {
                            Some(Command::SendText(message)) => {
                                let send_result = tokio::select! {
                                    _ = shutdown.changed() => return,
                                    result = socket.send(Frame::text(message)) => result,
                                };
                                if let Err(error) = send_result {
                                    if !report_failure(&events, &mut shutdown, error.to_string()).await {
                                        return;
                                    }
                                    break 'connected;
                                }
                            }
                            Some(Command::Close) => {
                                tokio::select! {
                                    _ = shutdown.changed() => return,
                                    _ = socket.send(Frame::close(CloseCode::Normal, b"")) => {}
                                }
                                if !publish(&events, &mut shutdown, Event::Closed).await {
                                    return;
                                }
                                break 'connected;
                            }
                            Some(Command::Connect(next_url)) => {
                                tokio::select! {
                                    _ = shutdown.changed() => return,
                                    _ = socket.send(Frame::close(CloseCode::Normal, b"")) => {}
                                }
                                if !publish(&events, &mut shutdown, Event::Closed).await {
                                    return;
                                }
                                pending_command = Some(Command::Connect(next_url));
                                break 'connected;
                            }
                            None => return,
                        }
                    }
                }
            }
            Command::SendText(_) => {
                if !publish(
                    &events,
                    &mut shutdown,
                    Event::Error("cannot send text while disconnected".to_owned()),
                )
                .await
                {
                    return;
                }
            }
            Command::Close => {
                if !publish(&events, &mut shutdown, Event::Closed).await {
                    return;
                }
            }
        }
    }
}

async fn report_failure(
    events: &mpsc::Sender<Event>,
    shutdown: &mut watch::Receiver<bool>,
    message: String,
) -> bool {
    publish(events, shutdown, Event::Error(message)).await
        && publish(events, shutdown, Event::Closed).await
}

async fn publish(
    events: &mpsc::Sender<Event>,
    shutdown: &mut watch::Receiver<bool>,
    event: Event,
) -> bool {
    tokio::select! {
        _ = shutdown.changed() => false,
        result = events.send(event) => result.is_ok(),
    }
}
