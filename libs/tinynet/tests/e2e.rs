use std::convert::Infallible;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use bytes::Bytes;
use futures::{StreamExt, stream};
use http_body_util::Empty;
use hyper::Request;
use rcgen::generate_simple_self_signed;
use tinynet::{Error, send_request, stream_request};

type AppState = &'static str;

fn app(state: AppState) -> Router {
    Router::new()
        .route("/mock", get(mock_handler))
        .route("/stream", get(stream_handler))
        .route("/error", get(error_handler))
        .with_state(state)
}

async fn mock_handler(State(body): State<AppState>) -> impl IntoResponse {
    (StatusCode::OK, body)
}

async fn stream_handler() -> impl IntoResponse {
    let chunks = vec![
        Ok::<_, Infallible>(Bytes::from("first-")),
        Ok::<_, Infallible>(Bytes::from("second-")),
        Ok::<_, Infallible>(Bytes::from("third")),
    ];

    let stream = stream::iter(chunks).then(|item| async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        item
    });

    Body::from_stream(stream)
}

async fn error_handler() -> impl IntoResponse {
    (StatusCode::BAD_GATEWAY, "upstream failed")
}

async fn start_server(state: AppState) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = app(state);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{addr}")
}

async fn start_tls_server(state: AppState) -> String {
    let cert = generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_pem = cert.cert.pem();
    let key_pem = cert.signing_key.serialize_pem();

    let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem(
        cert_pem.into_bytes(),
        key_pem.into_bytes(),
    )
    .await
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = app(state);

    tokio::spawn(async move {
        axum_server::from_tcp_rustls(listener.into_std().unwrap(), tls_config)
            .serve(app.into_make_service())
            .await
            .unwrap();
    });

    format!("https://localhost:{}", addr.port())
}

#[tokio::test]
async fn send_request_reads_mock_body() {
    let base_url = start_server("mock response body").await;
    let req = Request::builder()
        .uri(format!("{base_url}/mock"))
        .body(Empty::<Bytes>::new())
        .unwrap();

    let (status, body) = send_request(req).await.unwrap();

    assert_eq!(status, 200);
    assert_eq!(body, Bytes::from_static(b"mock response body"));
}

#[tokio::test]
async fn stream_request_yields_all_chunks() {
    let base_url = start_server("unused").await;
    let req = Request::builder()
        .uri(format!("{base_url}/stream"))
        .body(Empty::<Bytes>::new())
        .unwrap();

    let mut stream = stream_request(req).await;
    let mut collected = Vec::new();

    while let Some(next) = stream.next().await {
        collected.push(next.unwrap());
    }

    assert_eq!(
        collected,
        vec![
            Bytes::from_static(b"first-"),
            Bytes::from_static(b"second-"),
            Bytes::from_static(b"third"),
        ]
    );
}

#[tokio::test]
async fn stream_request_returns_server_error_with_body() {
    let base_url = start_server("unused").await;
    let req = Request::builder()
        .uri(format!("{base_url}/error"))
        .body(Empty::<Bytes>::new())
        .unwrap();

    let mut stream = stream_request(req).await;
    let first = stream.next().await.unwrap();

    match first {
        Err(Error::ServerError(status, message)) => {
            assert_eq!(status, 502);
            assert_eq!(message, "upstream failed");
        }
        other => panic!("unexpected result: {other:?}"),
    }

    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn send_request_reads_mock_body_over_tls_with_disabled_validation() {
    let base_url = start_tls_server("secure mock body").await;
    let _guard = InsecureTlsGuard::set();

    let req = Request::builder()
        .uri(format!("{base_url}/mock"))
        .body(Empty::<Bytes>::new())
        .unwrap();

    let (status, body) = send_request(req).await.unwrap();

    assert_eq!(status, 200);
    assert_eq!(body, Bytes::from_static(b"secure mock body"));
}

struct InsecureTlsGuard;

impl InsecureTlsGuard {
    fn set() -> Self {
        unsafe { std::env::set_var("TINYNET_INSECURE_TLS", "1") };
        Self
    }
}

impl Drop for InsecureTlsGuard {
    fn drop(&mut self) {
        unsafe { std::env::remove_var("TINYNET_INSECURE_TLS") };
    }
}
