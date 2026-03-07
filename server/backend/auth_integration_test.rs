use std::time::Duration;

use friday::api::auth;
use friday::grpc::generated::friday::core::rpc::{
    HelloRequest, hello_service_client::HelloServiceClient,
};
use tonic::Request;
use tonic::transport::Channel;
use uuid::Uuid;

async fn register_with_retry(
    endpoint: &str,
    email: &str,
    password: &str,
) -> Result<auth::AuthSession, tonic::Status> {
    let mut last_err = None;
    for _ in 0..30 {
        match auth::register(endpoint, email, password).await {
            Ok(session) => return Ok(session),
            Err(err) => {
                last_err = Some(err);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| tonic::Status::unavailable("server did not start")))
}

#[tokio::test]
async fn register_login_logout_flow_enforces_jwt_protection() {
    let grpc_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind grpc test port");
    let addr = grpc_listener.local_addr().expect("grpc local addr");
    drop(grpc_listener);

    let http_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind http test port");
    let http_addr = http_listener.local_addr().expect("http local addr");
    drop(http_listener);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server_task = tokio::spawn(async move {
        friday_backend::run_server_with_shutdown(
            addr,
            http_addr,
            "sqlite::memory:",
            "test-secret".to_string(),
            std::env::temp_dir().to_string_lossy().into_owned(),
            std::env::temp_dir().to_string_lossy().into_owned(),
            async move {
                let _ = shutdown_rx.await;
            },
        )
        .await
    });

    let endpoint = format!("http://{}", addr);
    let email = format!("user-{}@example.com", Uuid::new_v4());
    let password = "secret-password-123";

    let _registration = register_with_retry(&endpoint, &email, password)
        .await
        .expect("register");
    let login_session = auth::login(&endpoint, &email, password)
        .await
        .expect("login");

    let channel = Channel::from_shared(endpoint.clone())
        .expect("valid endpoint")
        .connect()
        .await
        .expect("connect hello client");
    let mut hello_client = HelloServiceClient::new(channel);

    let mut hello_request = Request::new(HelloRequest {
        name: "integration".to_string(),
    });
    hello_request.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", login_session.token)
            .parse()
            .expect("authorization metadata"),
    );
    let hello_response = hello_client
        .say_hello(hello_request)
        .await
        .expect("hello allowed");
    assert!(hello_response.into_inner().message.contains("integration"));

    let logout_ok = auth::logout(&endpoint, &login_session.token)
        .await
        .expect("logout");
    assert!(logout_ok);

    let mut revoked_request = Request::new(HelloRequest {
        name: "integration".to_string(),
    });
    revoked_request.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", login_session.token)
            .parse()
            .expect("authorization metadata"),
    );
    let revoked_err = hello_client
        .say_hello(revoked_request)
        .await
        .expect_err("revoked token must be rejected");
    assert_eq!(revoked_err.code(), tonic::Code::Unauthenticated);

    let _ = shutdown_tx.send(());
    server_task
        .await
        .expect("server join")
        .expect("server shutdown");
}
