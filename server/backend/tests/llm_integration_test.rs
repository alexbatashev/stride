use std::time::Duration;

use friday::api::auth;
use friday::grpc::generated::friday::core::rpc::language_model_client::LanguageModelClient;
use friday::grpc::generated::friday::core::rpc::{
    Empty, ProviderKind, RegisterModelRequest, RegisterProviderRequest,
};
use minisql::{ConnectionPool, Value};
use tonic::Request;
use tonic::transport::Channel;
use uuid::Uuid;

async fn connect_client_with_retry(endpoint: &str) -> LanguageModelClient<Channel> {
    let mut last_err = None;
    for _ in 0..30 {
        match Channel::from_shared(endpoint.to_owned()) {
            Ok(channel) => match channel.connect().await {
                Ok(channel) => return LanguageModelClient::new(channel),
                Err(err) => last_err = Some(err.to_string()),
            },
            Err(err) => last_err = Some(err.to_string()),
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!(
        "failed to connect to language model service: {:?}",
        last_err
    );
}

fn auth_request<T>(body: T, token: &str) -> Request<T> {
    let mut request = Request::new(body);
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {token}")
            .parse()
            .expect("authorization metadata"),
    );
    request
}

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
async fn register_provider_and_model_and_list_are_user_scoped_and_encrypted() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test port");
    let addr = listener.local_addr().expect("local addr");
    drop(listener);

    let db_path = std::env::temp_dir().join(format!("friday-llm-{}.db", Uuid::new_v4()));
    let db_url = format!("sqlite:{}", db_path.to_string_lossy());

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server_task = tokio::spawn(async move {
        friday_backend::run_server_with_shutdown(
            addr,
            &db_url,
            "test-secret".to_string(),
            async move {
                let _ = shutdown_rx.await;
            },
        )
        .await
    });

    let endpoint = format!("http://{}", addr);
    let owner_email = format!("owner-{}@example.com", Uuid::new_v4());
    let other_email = format!("other-{}@example.com", Uuid::new_v4());
    let owner_session = register_with_retry(&endpoint, &owner_email, "secret-password-123")
        .await
        .expect("register owner");
    let other_session = auth::register(&endpoint, &other_email, "secret-password-123")
        .await
        .expect("register other user");

    let mut owner_client = connect_client_with_retry(&endpoint).await;
    let mut other_client = connect_client_with_retry(&endpoint).await;

    let provider_id = Uuid::new_v4();
    let provider = RegisterProviderRequest {
        provider_id: provider_id.to_string(),
        provider_name: "OpenAI Main".to_owned(),
        kind: ProviderKind::OpenaiLike as i32,
        api_base_url: "https://api.openai.com".to_owned(),
        token: "sk-test-value".to_owned(),
    };
    owner_client
        .register_provider(auth_request(provider, &owner_session.token))
        .await
        .expect("register provider");

    let model = RegisterModelRequest {
        provider_id: provider_id.to_string(),
        model_slug: "gpt-4o-mini".to_owned(),
        model_name: "GPT-4o Mini".to_owned(),
        supports_thinking: true,
        supports_tools: true,
        supports_image_input: true,
    };
    owner_client
        .register_model(auth_request(model, &owner_session.token))
        .await
        .expect("register model");

    let owner_list = owner_client
        .list(auth_request(Empty {}, &owner_session.token))
        .await
        .expect("owner list models")
        .into_inner();
    assert_eq!(owner_list.models.len(), 1);
    assert_eq!(owner_list.models[0].provider_id, provider_id.to_string());

    let other_list = other_client
        .list(auth_request(Empty {}, &other_session.token))
        .await
        .expect("other list models")
        .into_inner();
    assert!(other_list.models.is_empty());

    let db = ConnectionPool::new(&format!("sqlite:{}", db_path.to_string_lossy())).expect("db");
    let stored = db
        .query_with_params(
            "SELECT token_nonce, token_ciphertext, user_id FROM llm_providers WHERE id = ?",
            vec![Value::Uuid(provider_id)],
        )
        .await
        .expect("query providers");
    assert_eq!(stored.row_count(), 1);
    let row = &stored.rows()[0];
    let nonce = row.get_text("token_nonce").expect("nonce");
    let ciphertext = row.get_text("token_ciphertext").expect("ciphertext");
    assert!(!nonce.is_empty());
    assert!(!ciphertext.is_empty());
    assert_ne!(ciphertext, "sk-test-value");
    assert!(row.get("user_id").is_some());

    let _ = shutdown_tx.send(());
    server_task
        .await
        .expect("server join")
        .expect("server shutdown");

    let _ = std::fs::remove_file(&db_path);
}
