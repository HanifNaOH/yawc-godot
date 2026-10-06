use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::SinkExt;
use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use rcgen::generate_simple_self_signed;
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer},
};
use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use yawc::{Frame, Options, WebSocket, frame::OpCode};

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn local_wss_negotiates_permessage_deflate_and_round_trips_text() {
    let certificate = generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let certificate_der = certificate.cert.der().clone();
    let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certificate.signing_key.serialize_der(),
    ));

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate_der.clone()], private_key)
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(server_config));

    let mut trusted_roots = RootCertStore::empty();
    trusted_roots.add(certificate_der).unwrap();
    let client_config = ClientConfig::builder()
        .with_root_certificates(trusted_roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(client_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (negotiated_tx, negotiated_rx) = oneshot::channel();
    let (session_tx, mut session_rx) = mpsc::channel(1);
    let negotiated_tx = Arc::new(Mutex::new(Some(negotiated_tx)));
    let client_message = format!("client payload: {}", "yawc-compression-test/".repeat(128));
    let server_message = format!("server payload: {}", "compressed-server-reply/".repeat(128));
    let expected_client_message = client_message.clone();
    let response_message = server_message.clone();

    let server_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let tls_stream = acceptor.accept(stream).await.unwrap();

        let service = service_fn(move |mut request: Request<Incoming>| {
            let negotiated_tx = Arc::clone(&negotiated_tx);
            let session_tx = session_tx.clone();
            let expected_client_message = expected_client_message.clone();
            let response_message = response_message.clone();

            async move {
                let (response, upgrade) = WebSocket::upgrade_with_options(
                    &mut request,
                    Options::default().with_balanced_compression(),
                )
                .expect("websocket upgrade failed");

                let negotiated_extensions = response
                    .headers()
                    .get("sec-websocket-extensions")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_owned();
                if let Some(sender) = negotiated_tx.lock().unwrap().take() {
                    let _ = sender.send(negotiated_extensions);
                }

                let session = tokio::spawn(async move {
                    let mut websocket = upgrade.await.expect("websocket upgrade failed");
                    let received = websocket.next_frame().await.expect("client frame missing");
                    assert_eq!(received.opcode(), OpCode::Text);
                    assert_eq!(received.as_str(), expected_client_message);

                    websocket
                        .send(Frame::text(response_message))
                        .await
                        .expect("server text send failed");

                    let close = websocket.next_frame().await.expect("close frame missing");
                    assert_eq!(close.opcode(), OpCode::Close);
                });
                session_tx
                    .send(session)
                    .await
                    .expect("test receiver dropped");

                Ok::<_, Infallible>(response)
            }
        });

        http1::Builder::new()
            .serve_connection(TokioIo::new(tls_stream), service)
            .with_upgrades()
            .await
            .expect("HTTP/WebSocket server connection failed");
    });

    let url = format!("wss://localhost:{}/", address.port())
        .parse()
        .unwrap();
    let mut client = tokio::time::timeout(
        TEST_TIMEOUT,
        WebSocket::connect(url)
            .with_options(Options::default().with_balanced_compression())
            .with_connector(connector)
            .with_tcp_address(address),
    )
    .await
    .expect("WSS connection timed out")
    .expect("WSS connection failed");

    let negotiated_extensions = tokio::time::timeout(TEST_TIMEOUT, negotiated_rx)
        .await
        .expect("handshake result timed out")
        .expect("server did not report the handshake");
    assert!(
        negotiated_extensions.contains("permessage-deflate"),
        "server did not negotiate compression: {negotiated_extensions}"
    );

    client.send(Frame::text(client_message)).await.unwrap();
    let received = tokio::time::timeout(TEST_TIMEOUT, client.next_frame())
        .await
        .expect("compressed server message timed out")
        .expect("compressed server message missing");
    assert_eq!(received.opcode(), OpCode::Text);
    assert_eq!(received.as_str(), server_message);

    client
        .send(Frame::close(yawc::close::CloseCode::Normal, b""))
        .await
        .unwrap();

    let session = tokio::time::timeout(TEST_TIMEOUT, session_rx.recv())
        .await
        .expect("server session timed out")
        .expect("server session missing");
    tokio::time::timeout(TEST_TIMEOUT, session)
        .await
        .expect("server session did not close")
        .expect("server session panicked");
    tokio::time::timeout(TEST_TIMEOUT, server_task)
        .await
        .expect("server connection did not close")
        .expect("server task panicked");
}
