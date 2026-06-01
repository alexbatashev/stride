use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use friday_agent::{
    AgentConfig, ModelRegistry,
    mcp::{self, McpServer},
};
use serde_json::{Value, json};

const SESSION_ID: &str = "test-session";

#[derive(Clone)]
struct MockState;

async fn mcp_endpoint(
    State(_): State<Arc<MockState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let request: Value = serde_json::from_str(&body).unwrap();
    let method = request.get("method").and_then(Value::as_str).unwrap();
    let id = request.get("id").cloned();

    // Notifications carry no id and expect an empty 202.
    let Some(id) = id else {
        return StatusCode::ACCEPTED.into_response();
    };

    // Every request after the handshake must carry the assigned session id.
    if method != "initialize" {
        let session = headers.get("mcp-session-id").and_then(|v| v.to_str().ok());
        assert_eq!(session, Some(SESSION_ID), "missing session id on {method}");
    }

    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "mock", "version": "0.1.0" }
        }),
        "tools/list" => json!({
            "tools": [{
                "name": "echo",
                "description": "Echo the provided text",
                "inputSchema": {
                    "type": "object",
                    "properties": { "text": { "type": "string", "description": "Text to echo" } },
                    "required": ["text"]
                }
            }]
        }),
        "tools/call" => {
            let text = request
                .pointer("/params/arguments/text")
                .and_then(Value::as_str)
                .unwrap_or("");
            json!({ "content": [{ "type": "text", "text": format!("echoed: {text}") }] })
        }
        other => panic!("unexpected method: {other}"),
    };

    let response = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    let mut http = axum::Json(response).into_response();
    http.headers_mut()
        .insert("mcp-session-id", SESSION_ID.parse().unwrap());
    http
}

async fn spawn_mock_server() -> String {
    let app = Router::new()
        .route("/mcp", post(mcp_endpoint))
        .with_state(Arc::new(MockState));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}/mcp")
}

#[tokio::test(flavor = "multi_thread")]
async fn connects_lists_and_calls_mcp_tools() {
    let url = spawn_mock_server().await;

    let tools = mcp::connect(
        "mock",
        McpServer {
            url,
            headers: Vec::new(),
        },
    )
    .await
    .unwrap();

    assert_eq!(tools.len(), 1);

    let tool = &tools[0];
    assert_eq!(friday_agent::Tool::name(tool), "mock_echo");
    let definition = friday_agent::Tool::definition(tool);
    let params = definition.function.parameters.unwrap();
    assert_eq!(params.required, Some(vec!["text".to_string()]));
    assert_eq!(params.properties["text"].r#type, "string");

    let config = Arc::new(AgentConfig {
        model_registry: ModelRegistry::new(),
        max_iterations: 0,
    });
    let result = friday_agent::Tool::execute(tool, config, json!({ "text": "hello" })).await;

    assert_eq!(
        result.pointer("/content/0/text").and_then(Value::as_str),
        Some("echoed: hello")
    );
}

async fn sse_endpoint(headers: HeaderMap, body: String) -> Response {
    let request: Value = serde_json::from_str(&body).unwrap();
    let Some(id) = request.get("id").cloned() else {
        return StatusCode::ACCEPTED.into_response();
    };
    let method = request.get("method").and_then(Value::as_str).unwrap();
    if method != "initialize" {
        assert!(headers.contains_key("mcp-session-id"));
    }

    let result = match method {
        "initialize" => json!({ "protocolVersion": "2025-06-18", "capabilities": {} }),
        "tools/list" => json!({
            "tools": [{ "name": "ping", "description": "Ping", "inputSchema": { "type": "object" } }]
        }),
        _ => panic!("unexpected method: {method}"),
    };

    let message = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    let payload = format!("event: message\ndata: {message}\n\n");
    Response::builder()
        .header("content-type", "text/event-stream")
        .header("mcp-session-id", SESSION_ID)
        .body(payload.into())
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn parses_sse_framed_responses() {
    let app = Router::new().route("/mcp", post(sse_endpoint));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let tools = mcp::connect(
        "remote",
        McpServer {
            url: format!("http://{addr}/mcp"),
            headers: Vec::new(),
        },
    )
    .await
    .unwrap();

    assert_eq!(tools.len(), 1);
    assert_eq!(friday_agent::Tool::name(&tools[0]), "remote_ping");
}
