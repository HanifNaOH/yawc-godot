use std::{
    thread::{self, JoinHandle},
    time::Duration,
};

use futures_util::SinkExt;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{mpsc, watch},
};
use yawc::{Frame, Options, WebSocket, close::CloseCode, frame::OpCode};

const COMMAND_CAPACITY: usize = 64;
const EVENT_CAPACITY: usize = 256;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);
const CLOSE_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_INBOUND_MESSAGE_BYTES: usize = 1024 * 1024;

pub(crate) enum Command {
    Connect(String),
    SendText(String),
    SendBinary(Vec<u8>),
    Close,
}

pub(crate) enum Event {
    Opened,
    Closed,
    Error(String),
    TextMessage(String),
    BinaryMessage(Vec<u8>),
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

    pub(crate) fn restart(&mut self) {
        let is_running = self
            .thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished());
        if is_running {
            return;
        }

        self.shutdown();
        *self = Self::start();
    }

    pub(crate) fn try_recv(&mut self) -> Result<Event, mpsc::error::TryRecvError> {
        self.events.try_recv()
    }

    pub(crate) fn shutdown(&mut self) {
        let _ = self.shutdown.send(true);
        // Dropping a JoinHandle detaches the worker instead of blocking the Godot thread.
        drop(self.thread.take());
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
                    WebSocket::connect(parsed_url).with_options(transport_options()),
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
                            Some(Command::SendText(_)) | Some(Command::SendBinary(_)) => {
                                if !publish(
                                    &events,
                                    &mut shutdown,
                                    Event::Error("cannot send before the connection opens".to_owned()),
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
                    finish_close_handshake(&mut socket).await;
                    return;
                }

                let mut shutdown_requested = false;
                'connected: loop {
                    tokio::select! {
                        _ = shutdown.changed() => {
                            shutdown_requested = true;
                            break 'connected;
                        },
                        frame = socket.next_frame() => match frame {
                            Ok(frame) if frame.opcode() == OpCode::Text => {
                                let message = match String::from_utf8(frame.payload().to_vec()) {
                                    Ok(message) => message,
                                    Err(error) => {
                                        if !report_failure(
                                            &events,
                                            &mut shutdown,
                                            format!("invalid UTF-8 text frame: {error}"),
                                        )
                                        .await
                                        {
                                            shutdown_requested = true;
                                        }
                                        break 'connected;
                                    }
                                };
                                if !publish(
                                    &events,
                                    &mut shutdown,
                                    Event::TextMessage(message),
                                ).await {
                                    shutdown_requested = true;
                                    break 'connected;
                                }
                            }
                            Ok(frame) if frame.opcode() == OpCode::Binary => {
                                if !publish(
                                    &events,
                                    &mut shutdown,
                                    Event::BinaryMessage(frame.payload().to_vec()),
                                ).await {
                                    shutdown_requested = true;
                                    break 'connected;
                                }
                            }
                            Ok(frame) if frame.opcode() == OpCode::Close => {
                                flush_close_reply(&mut socket).await;
                                if !publish(&events, &mut shutdown, Event::Closed).await {
                                    return;
                                }
                                break 'connected;
                            }
                            Ok(_) => {}
                            Err(error) => {
                                if !report_failure(&events, &mut shutdown, error.to_string()).await {
                                    shutdown_requested = true;
                                }
                                break 'connected;
                            }
                        },
                        command = commands.recv() => match command {
                            Some(Command::SendText(message)) => {
                                let send_result = tokio::select! {
                                    _ = shutdown.changed() => None,
                                    result = socket.send(Frame::text(message)) => Some(result),
                                };
                                let Some(send_result) = send_result else {
                                    shutdown_requested = true;
                                    break 'connected;
                                };
                                if let Err(error) = send_result {
                                    if !report_failure(&events, &mut shutdown, error.to_string()).await {
                                        shutdown_requested = true;
                                    }
                                    break 'connected;
                                }
                            }
                            Some(Command::SendBinary(message)) => {
                                let send_result = tokio::select! {
                                    _ = shutdown.changed() => None,
                                    result = socket.send(Frame::binary(message)) => Some(result),
                                };
                                let Some(send_result) = send_result else {
                                    shutdown_requested = true;
                                    break 'connected;
                                };
                                if let Err(error) = send_result {
                                    if !report_failure(&events, &mut shutdown, error.to_string()).await {
                                        shutdown_requested = true;
                                    }
                                    break 'connected;
                                }
                            }
                            Some(Command::Close) => {
                                finish_close_handshake(&mut socket).await;
                                if !publish(&events, &mut shutdown, Event::Closed).await {
                                    return;
                                }
                                break 'connected;
                            }
                            Some(Command::Connect(next_url)) => {
                                finish_close_handshake(&mut socket).await;
                                if !publish(&events, &mut shutdown, Event::Closed).await {
                                    return;
                                }
                                pending_command = Some(Command::Connect(next_url));
                                break 'connected;
                            }
                            None => {
                                shutdown_requested = true;
                                break 'connected;
                            }
                        }
                    }
                }

                if shutdown_requested {
                    finish_close_handshake(&mut socket).await;
                    return;
                }
            }
            Command::SendText(_) | Command::SendBinary(_) => {
                if !publish(
                    &events,
                    &mut shutdown,
                    Event::Error("cannot send while disconnected".to_owned()),
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

async fn finish_close_handshake<S>(socket: &mut WebSocket<S>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let _ = tokio::time::timeout(CLOSE_HANDSHAKE_TIMEOUT, async {
        if socket
            .send(Frame::close(CloseCode::Normal, b""))
            .await
            .is_err()
        {
            return;
        }

        loop {
            match socket.next_frame().await {
                Ok(frame) if frame.opcode() == OpCode::Close => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
    })
    .await;
}

fn transport_options() -> Options {
    Options::default()
        .with_limits(MAX_INBOUND_MESSAGE_BYTES, MAX_INBOUND_MESSAGE_BYTES)
        .with_utf8()
}

async fn flush_close_reply<S>(socket: &mut WebSocket<S>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let _ = tokio::time::timeout(CLOSE_HANDSHAKE_TIMEOUT, socket.next_frame()).await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use tokio::net::TcpListener;

    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    #[tokio::test]
    async fn worker_disables_compression_and_shutdown_finishes_close_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (close_seen_tx, mut close_seen_rx) = mpsc::unbounded_channel();
        let (compression_offered_tx, mut compression_offered_rx) = mpsc::unbounded_channel();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let service = service_fn(move |mut request: Request<Incoming>| {
                let close_seen_tx = close_seen_tx.clone();
                let compression_offered_tx = compression_offered_tx.clone();
                async move {
                    let compression_offered =
                        request.headers().contains_key("sec-websocket-extensions");
                    let _ = compression_offered_tx.send(compression_offered);
                    let (response, upgrade) = WebSocket::upgrade_with_options(
                        &mut request,
                        Options::default().with_balanced_compression(),
                    )
                    .unwrap();
                    tokio::spawn(async move {
                        let Ok(mut websocket) = upgrade.await else {
                            let _ = close_seen_tx.send(false);
                            return;
                        };
                        let close_received = websocket
                            .next_frame()
                            .await
                            .is_ok_and(|frame| frame.opcode() == OpCode::Close);
                        let _ = close_seen_tx.send(close_received);
                        if close_received {
                            let _ = websocket.next_frame().await;
                        }
                    });
                    Ok::<_, Infallible>(response)
                }
            });

            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades()
                .await
                .unwrap();
        });

        let (command_tx, command_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (event_tx, mut event_rx) = mpsc::channel(EVENT_CAPACITY);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker = tokio::spawn(run_worker(command_rx, event_tx, shutdown_rx));

        assert!(
            command_tx
                .send(Command::Connect(format!("ws://{address}/")))
                .await
                .is_ok()
        );
        let opened = tokio::time::timeout(TEST_TIMEOUT, async {
            loop {
                match event_rx.recv().await {
                    Some(Event::Opened) => break true,
                    Some(Event::Error(message)) => panic!("connection failed: {message}"),
                    Some(_) => {}
                    None => break false,
                }
            }
        })
        .await
        .expect("worker did not open the test connection");
        assert!(opened);
        let compression_offered = tokio::time::timeout(TEST_TIMEOUT, compression_offered_rx.recv())
            .await
            .expect("server did not inspect the extension offer")
            .unwrap();
        assert!(
            !compression_offered,
            "transport must not negotiate compression"
        );
        let options = transport_options();
        assert!(options.check_utf8, "incoming text must be UTF-8 validated");
        assert_eq!(options.max_payload_read, Some(MAX_INBOUND_MESSAGE_BYTES));
        assert_eq!(options.max_read_buffer, Some(MAX_INBOUND_MESSAGE_BYTES));
        assert!(options.compression.is_none());

        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(TEST_TIMEOUT, worker)
            .await
            .expect("worker did not stop")
            .unwrap();
        assert!(
            tokio::time::timeout(TEST_TIMEOUT, close_seen_rx.recv())
                .await
                .expect("server did not receive a close frame")
                .unwrap(),
            "worker shutdown must send a WebSocket Close frame"
        );
        tokio::time::timeout(TEST_TIMEOUT, server)
            .await
            .expect("test server did not stop")
            .unwrap();
    }

    #[tokio::test]
    async fn invalid_utf8_text_emits_error_and_closed() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let service = service_fn(|mut request: Request<Incoming>| async move {
                let (response, upgrade) = WebSocket::upgrade(&mut request).unwrap();
                tokio::spawn(async move {
                    if let Ok(mut websocket) = upgrade.await {
                        let _ = websocket.send(Frame::text(vec![0xff])).await;
                    }
                });
                Ok::<_, Infallible>(response)
            });

            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades()
                .await
                .unwrap();
        });

        let (command_tx, command_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (event_tx, mut event_rx) = mpsc::channel(EVENT_CAPACITY);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker = tokio::spawn(run_worker(command_rx, event_tx, shutdown_rx));
        assert!(
            command_tx
                .send(Command::Connect(format!("ws://{address}/")))
                .await
                .is_ok()
        );

        let (got_error, got_closed) = tokio::time::timeout(TEST_TIMEOUT, async {
            let mut got_error = false;
            let mut got_closed = false;
            while !got_closed {
                match event_rx.recv().await {
                    Some(Event::Error(message)) => {
                        assert!(!message.is_empty());
                        got_error = true;
                    }
                    Some(Event::Closed) => got_closed = true,
                    Some(_) => {}
                    None => break,
                }
            }
            (got_error, got_closed)
        })
        .await
        .expect("invalid text frame did not produce terminal signals");
        assert!(got_error, "invalid UTF-8 should emit an error");
        assert!(got_closed, "invalid UTF-8 should close the connection");

        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(TEST_TIMEOUT, worker)
            .await
            .expect("worker did not stop")
            .unwrap();
        tokio::time::timeout(TEST_TIMEOUT, server)
            .await
            .expect("test server did not stop")
            .unwrap();
    }

    #[test]
    fn worker_shutdown_does_not_join_on_the_calling_thread() {
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker_thread = thread::spawn(move || {
            started_tx.send(()).unwrap();
            let _ = release_rx.recv();
        });
        started_rx.recv().unwrap();

        let (commands, _command_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (_event_tx, events) = mpsc::channel(EVENT_CAPACITY);
        let (shutdown, _shutdown_rx) = watch::channel(false);
        let worker = Worker {
            commands,
            events,
            shutdown,
            thread: Some(worker_thread),
        };
        let (returned_tx, returned_rx) = std::sync::mpsc::channel();
        let caller = thread::spawn(move || {
            let mut worker = worker;
            worker.shutdown();
            let _ = returned_tx.send(());
        });

        let returned = returned_rx.recv_timeout(Duration::from_millis(200)).is_ok();
        let _ = release_tx.send(());
        caller.join().unwrap();
        assert!(returned, "shutdown must not wait for the worker thread");
    }
}
