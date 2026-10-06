use std::convert::Infallible;

use futures_util::SinkExt;
use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use yawc::{Frame, Options, WebSocket, frame::OpCode};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let address = "127.0.0.1:18765";
    let listener = TcpListener::bind(address).await.expect("bind echo server");
    println!("listening on ws://{address}");

    loop {
        let (stream, _) = listener.accept().await.expect("accept client");
        tokio::spawn(async move {
            let service = service_fn(|mut request: Request<Incoming>| async move {
                let (response, upgrade) = WebSocket::upgrade_with_options(
                    &mut request,
                    Options::default().with_balanced_compression(),
                )
                .expect("upgrade WebSocket request");

                tokio::spawn(async move {
                    let Ok(mut websocket) = upgrade.await else {
                        return;
                    };

                    loop {
                        match websocket.next_frame().await {
                            Ok(frame) if frame.opcode() == OpCode::Text => {
                                let reply = format!("echo:{}", frame.as_str());
                                if websocket.send(Frame::text(reply)).await.is_err() {
                                    break;
                                }
                            }
                            Ok(frame) if frame.opcode() == OpCode::Close => break,
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    }
                });

                Ok::<_, Infallible>(response)
            });

            let _ = http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades()
                .await;
        });
    }
}
