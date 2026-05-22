use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, State};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{HeaderMap, Method, Response, StatusCode};
use axum::routing::{get, post};
use bytes::Bytes;
use futures::{StreamExt, stream};
use http_body_util::Full;
use hyper::Request;
use llm::{CompletionRequest, Error, Message, ModelDesc, OpenAI, Role};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

#[derive(Clone)]
struct TestServerState {
    client_token: String,
    upstream: Upstream,
}

#[derive(Clone)]
enum Upstream {
    Generated { model: String },
    Live(LiveProvider),
}

#[derive(Clone)]
struct LiveProvider {
    base_url: String,
    token: String,
    model: String,
}

struct TestServer {
    base_url: String,
    client_token: String,
    model: String,
}

#[derive(Deserialize)]
struct ModelListResponse {
    data: Vec<ModelDesc>,
}

#[derive(Deserialize)]
struct ChatRequest {
    model: String,
    stream: Option<bool>,
}

#[derive(Serialize)]
struct ChatCompletionResponse {
    id: String,
    created: u32,
    model: String,
    choices: Vec<serde_json::Value>,
    usage: serde_json::Value,
}

fn live_provider() -> Option<LiveProvider> {
    let base_url = std::env::var("LLM_E2E_BASE_URL").ok()?;
    let token = std::env::var("LLM_E2E_TOKEN").ok()?;
    let model = std::env::var("LLM_E2E_MODEL").ok()?;
    Some(LiveProvider {
        base_url,
        token,
        model,
    })
}

fn app(state: TestServerState) -> Router {
    Router::new()
        .route("/v1/models", get(list_models))
        .route("/v1/models/{*model}", get(get_model))
        .route("/v1/chat/completions", post(chat_completion))
        .with_state(Arc::new(state))
}

async fn start_server(upstream: Upstream) -> TestServer {
    let client_token = Uuid::new_v4().to_string();
    let model = match &upstream {
        Upstream::Generated { model } => model.clone(),
        Upstream::Live(provider) => provider.model.clone(),
    };
    let state = TestServerState {
        client_token: client_token.clone(),
        upstream,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app(state)).await.unwrap();
    });

    TestServer {
        base_url: format!("http://{addr}"),
        client_token,
        model,
    }
}

async fn start_generated_server() -> TestServer {
    start_server(Upstream::Generated {
        model: Uuid::new_v4().to_string(),
    })
    .await
}

async fn start_live_server() -> Option<TestServer> {
    Some(start_server(Upstream::Live(live_provider()?)).await)
}

async fn list_models(
    State(state): State<Arc<TestServerState>>,
    headers: HeaderMap,
) -> Response<Body> {
    if let Err(res) = validate_auth(&state, &headers) {
        return res;
    }

    match &state.upstream {
        Upstream::Generated { model } => generated_models(model),
        Upstream::Live(provider) => {
            proxy_request(provider, Method::GET, "/v1/models", Bytes::new()).await
        }
    }
}

async fn get_model(
    State(state): State<Arc<TestServerState>>,
    Path(model): Path<String>,
    headers: HeaderMap,
) -> Response<Body> {
    if let Err(res) = validate_auth(&state, &headers) {
        return res;
    }

    match &state.upstream {
        Upstream::Generated { model: known_model } if known_model == &model => {
            generated_model(known_model)
        }
        Upstream::Generated { .. } => text_response(StatusCode::NOT_FOUND, "model not found"),
        Upstream::Live(provider) => get_live_model(provider, &model).await,
    }
}

async fn get_live_model(provider: &LiveProvider, model: &str) -> Response<Body> {
    let res = proxy_request(provider, Method::GET, "/v1/models", Bytes::new()).await;
    if !res.status().is_success() {
        return res;
    }

    let body = to_bytes(res.into_body(), 1_048_576).await.unwrap();
    let models: ModelListResponse = serde_json::from_slice(&body).unwrap();
    let Some(model) = models.data.into_iter().find(|item| item.id == model) else {
        return text_response(StatusCode::NOT_FOUND, "model not found");
    };

    json_response(StatusCode::OK, serde_json::to_vec(&model).unwrap())
}

async fn chat_completion(
    State(state): State<Arc<TestServerState>>,
    headers: HeaderMap,
    body: Body,
) -> Response<Body> {
    if let Err(res) = validate_auth(&state, &headers) {
        return res;
    }

    let body = to_bytes(body, 1_048_576).await.unwrap();
    let chat_request: ChatRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(err) => return text_response(StatusCode::BAD_REQUEST, &err.to_string()),
    };

    match &state.upstream {
        Upstream::Generated { model } if model != &chat_request.model => {
            text_response(StatusCode::NOT_FOUND, "model not found")
        }
        Upstream::Generated { model } if chat_request.stream.unwrap_or(false) => {
            generated_stream(model)
        }
        Upstream::Generated { model } => generated_completion(model),
        Upstream::Live(provider) if chat_request.stream.unwrap_or(false) => {
            proxy_stream(provider, "/v1/chat/completions", body).await
        }
        Upstream::Live(provider) => {
            proxy_request(provider, Method::POST, "/v1/chat/completions", body).await
        }
    }
}

fn validate_auth(state: &TestServerState, headers: &HeaderMap) -> Result<(), Response<Body>> {
    let expected = format!("Bearer {}", state.client_token);
    match headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        Some(actual) if actual == expected => Ok(()),
        _ => Err(text_response(
            StatusCode::UNAUTHORIZED,
            "invalid bearer token",
        )),
    }
}

fn generated_models(model: &str) -> Response<Body> {
    json_response(
        StatusCode::OK,
        serde_json::to_vec(&json!({
            "object": "list",
            "data": [model_desc(model)]
        }))
        .unwrap(),
    )
}

fn generated_model(model: &str) -> Response<Body> {
    json_response(
        StatusCode::OK,
        serde_json::to_vec(&model_desc(model)).unwrap(),
    )
}

fn model_desc(model: &str) -> serde_json::Value {
    json!({
        "id": model,
        "object": "model",
        "created": 1,
        "owned_by": "e2e",
        "supported_parameters": ["stream"]
    })
}

fn generated_completion(model: &str) -> Response<Body> {
    let completion = ChatCompletionResponse {
        id: Uuid::new_v4().to_string(),
        created: 1,
        model: model.to_string(),
        choices: vec![json!({
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "friday-llm-e2e"
            },
            "finish_reason": "stop"
        })],
        usage: json!({
            "prompt_tokens": 1,
            "completion_tokens": 1,
            "total_tokens": 2
        }),
    };
    json_response(StatusCode::OK, serde_json::to_vec(&completion).unwrap())
}

fn generated_stream(model: &str) -> Response<Body> {
    let id = Uuid::new_v4().to_string();
    let chunks = [
        json!({
            "id": id,
            "object": "chat.completion.chunk",
            "created": 1,
            "model": model,
            "system_fingerprint": null,
            "choices": [{
                "index": 0,
                "delta": {"content": "friday-"},
                "finish_reason": null
            }]
        }),
        json!({
            "id": id,
            "object": "chat.completion.chunk",
            "created": 1,
            "model": model,
            "system_fingerprint": null,
            "choices": [{
                "index": 0,
                "delta": {"content": "llm-e2e"},
                "finish_reason": "stop"
            }]
        }),
    ];
    let events = chunks
        .into_iter()
        .map(|chunk| format!("data: {}\n\n", serde_json::to_string(&chunk).unwrap()))
        .chain(std::iter::once("data: [DONE]\n\n".to_string()));

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/event-stream")
        .body(Body::from(events.collect::<String>()))
        .unwrap()
}

async fn proxy_request(
    provider: &LiveProvider,
    method: Method,
    path: &str,
    body: Bytes,
) -> Response<Body> {
    let req = match upstream_request(provider, method, path, body) {
        Ok(req) => req,
        Err(err) => return text_response(StatusCode::BAD_GATEWAY, &format!("{err:?}")),
    };

    match tinynet::send_request(req).await {
        Ok((status, body)) => json_response(StatusCode::from_u16(status).unwrap(), body.to_vec()),
        Err(err) => text_response(StatusCode::BAD_GATEWAY, &format!("{err:?}")),
    }
}

async fn proxy_stream(provider: &LiveProvider, path: &str, body: Bytes) -> Response<Body> {
    let req = match upstream_request(provider, Method::POST, path, body) {
        Ok(req) => req,
        Err(err) => return text_response(StatusCode::BAD_GATEWAY, &format!("{err:?}")),
    };

    let mut upstream = tinynet::stream_request(req).await;
    let Some(first) = upstream.next().await else {
        return text_response(StatusCode::BAD_GATEWAY, "upstream closed without data");
    };

    let first = match first {
        Ok(first) => first,
        Err(tinynet::Error::ServerError(status, message)) => {
            return text_response(StatusCode::from_u16(status).unwrap(), &message);
        }
        Err(err) => return text_response(StatusCode::BAD_GATEWAY, &format!("{err:?}")),
    };

    let stream = stream::once(async move { Ok::<Bytes, std::io::Error>(first) })
        .chain(upstream.map(|chunk| chunk.map_err(std::io::Error::other)));

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(stream))
        .unwrap()
}

fn upstream_request(
    provider: &LiveProvider,
    method: Method,
    path: &str,
    body: Bytes,
) -> Result<Request<Full<Bytes>>, Error> {
    Request::builder()
        .method(method)
        .uri(format!(
            "{}{}",
            provider.base_url.trim_end_matches('/'),
            path
        ))
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {}", provider.token))
        .body(Full::new(body))
        .map_err(|err| Error::InvalidRequest(err.to_string()))
}

fn json_response(status: StatusCode, body: Vec<u8>) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

fn text_response(status: StatusCode, body: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn request(model: &str) -> CompletionRequest {
    CompletionRequest::new(
        model,
        &[Message {
            role: Role::User,
            content: "Reply with exactly this token and no other text: friday-llm-e2e".to_string(),
            thinking: None,
            tool_calls: None,
            tool_call_id: None,
        }],
    )
    .max_tokens(32)
    .temperature(0.0)
}

fn rate_limited(err: &Error) -> bool {
    matches!(
        err,
        Error::ServerError(429) | Error::ServerErrorWithMessage(429, _)
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn openai_client_works_through_e2e_server() {
    tokio::time::timeout(Duration::from_secs(90), async {
        let server = start_generated_server().await;
        let client = OpenAI::new(&server.base_url);

        let models = client.list_models(&server.client_token).await.unwrap();
        assert!(
            models.iter().any(|item| item.id == server.model),
            "server did not list expected model"
        );

        let desc = client
            .get_model(&server.client_token, &server.model)
            .await
            .unwrap();
        assert_eq!(desc.id, server.model);

        let completion = match client
            .get_completion(&server.client_token, request(&server.model))
            .await
        {
            Ok(completion) => completion,
            Err(err) if rate_limited(&err) => return,
            Err(err) => panic!("completion failed: {err:?}"),
        };
        assert_eq!(completion.choices.len(), 1);
        assert_eq!(completion.choices[0].index, 0);
        assert!(
            completion.choices[0]
                .message
                .as_ref()
                .is_some_and(|message| !message.content.trim().is_empty())
        );
        assert!(completion.usage.total_tokens > 0);

        let mut stream =
            client.stream_completion(&server.client_token, request(&server.model).stream());
        let mut content = String::new();
        let mut saw_finish = false;

        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(err) if rate_limited(&err) => return,
                Err(err) => panic!("stream completion failed: {err:?}"),
            };
            for choice in chunk.choices {
                if let Some(delta) = choice.delta {
                    if let Some(delta_content) = delta.content {
                        content.push_str(&delta_content);
                    }
                }
                saw_finish |= choice.finish_reason.is_some();
            }
        }

        assert!(!content.trim().is_empty());
        assert!(saw_finish);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn openai_client_surfaces_server_errors_from_e2e_server() {
    let server = start_generated_server().await;
    let client = OpenAI::new(&server.base_url);
    let err = client
        .list_models(&Uuid::new_v4().to_string())
        .await
        .unwrap_err();

    match err {
        Error::ServerError(401) => {}
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn openai_client_can_use_live_provider_when_configured() {
    let Some(server) = start_live_server().await else {
        return;
    };
    let client = OpenAI::new(&server.base_url);

    let models = client.list_models(&server.client_token).await.unwrap();
    assert!(
        models.iter().any(|item| item.id == server.model),
        "live provider did not list configured model"
    );
}
