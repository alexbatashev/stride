//! Minimal client for external MCP servers over the Streamable HTTP transport.
//!
//! Connecting performs the JSON-RPC `initialize` handshake, lists the server's
//! tools, and wraps each one as a [`Tool`] that can be registered as searchable
//! on an agent. Calling a tool issues a `tools/call` request.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;
use llm::{Function, FunctionParameters, FunctionProperty, Tool as LlmTool};
use serde_json::{Map, Value, json};

use crate::{AgentConfig, Tool};

const PROTOCOL_VERSION: &str = "2025-06-18";

/// An external MCP server reachable over Streamable HTTP.
#[derive(Clone, Debug)]
pub struct McpServer {
    pub url: String,
    /// Extra request headers (e.g. `Authorization`).
    pub headers: Vec<(String, String)>,
}

struct McpClient {
    url: String,
    headers: Vec<(String, String)>,
    session_id: Option<String>,
}

/// A single tool advertised by an MCP server, exposed to the agent.
#[derive(Clone)]
pub struct McpTool {
    client: Arc<McpClient>,
    server_name: String,
    name: String,
    readable: String,
    remote_name: String,
    definition: LlmTool,
    /// Whether the server annotated this tool as read-only. Tools that may modify
    /// state (the MCP `readOnlyHint` default) are gated behind user approval.
    read_only: bool,
}

/// Connect to an MCP server and return one tool per advertised capability.
/// Exposed tool names are prefixed with `server` to avoid cross-server clashes.
pub async fn connect(server_name: &str, server: McpServer) -> Result<Vec<McpTool>, String> {
    let mut client = McpClient {
        url: server.url,
        headers: server.headers,
        session_id: None,
    };

    let (_, session_id) = client
        .rpc(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "stride", "version": env!("CARGO_PKG_VERSION") },
            }),
        )
        .await?;
    client.session_id = session_id;

    client
        .notify("notifications/initialized", json!({}))
        .await?;

    let (list, _) = client.rpc("tools/list", json!({})).await?;
    let tools = list
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let client = Arc::new(client);
    let mut result = Vec::new();
    for tool in tools {
        let Some(remote_name) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        let description = tool
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let parameters = tool
            .get("inputSchema")
            .cloned()
            .and_then(parameters_from_schema);
        let read_only = read_only_hint(&tool);
        let exposed = format!("{server_name}_{remote_name}");

        result.push(McpTool {
            client: client.clone(),
            server_name: server_name.to_string(),
            definition: LlmTool {
                r#type: llm::ToolType::Function,
                function: Function {
                    description,
                    name: exposed.clone(),
                    parameters,
                },
            },
            name: exposed,
            readable: remote_name.to_string(),
            remote_name: remote_name.to_string(),
            read_only,
        });
    }

    Ok(result)
}

#[async_trait(?Send)]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn readable_name(&self) -> &str {
        &self.readable
    }

    fn definition(&self) -> LlmTool {
        self.definition.clone()
    }

    fn searchable_category(&self) -> Option<String> {
        Some(format!("{} MCP", display_server_name(&self.server_name)))
    }

    /// State-changing tools (anything not annotated read-only) require explicit
    /// user approval before the call is forwarded to the server.
    fn requires_confirmation(&self) -> bool {
        !self.read_only
    }

    fn confirmation_prompt(&self, args: &Value) -> String {
        format!("Run MCP tool `{}` with arguments: {}", self.readable, args)
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = json!({ "name": self.remote_name, "arguments": args });
        match self.client.rpc("tools/call", params).await {
            Ok((result, _)) => result,
            Err(error) => json!({ "error": error }),
        }
    }
}

fn display_server_name(name: &str) -> String {
    name.split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(display_server_name_part)
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_server_name_part(part: &str) -> String {
    match part.to_ascii_lowercase().as_str() {
        "github" => "GitHub".to_string(),
        "gitlab" => "GitLab".to_string(),
        "gmail" => "Gmail".to_string(),
        "google" => "Google".to_string(),
        "jira" => "Jira".to_string(),
        "slack" => "Slack".to_string(),
        "figma" => "Figma".to_string(),
        _ if part.chars().any(char::is_uppercase) => part.to_string(),
        _ => {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

impl McpClient {
    /// Send a JSON-RPC request and return its `result` plus any session id the
    /// server assigned in the response.
    async fn rpc(&self, method: &str, params: Value) -> Result<(Value, Option<String>), String> {
        let payload = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        let (status, headers, body) = self.send(&payload).await?;
        let session = header_str(&headers, "mcp-session-id");

        if !(200..300).contains(&status) {
            return Err(format!(
                "MCP server returned status {status}: {}",
                String::from_utf8_lossy(&body)
            ));
        }

        let message = parse_response(&headers, &body)?;
        if let Some(error) = message.get("error") {
            return Err(error.to_string());
        }

        Ok((
            message.get("result").cloned().unwrap_or(Value::Null),
            session,
        ))
    }

    /// Send a JSON-RPC notification (no response expected).
    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let payload = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let (status, _, body) = self.send(&payload).await?;
        if !(200..300).contains(&status) {
            return Err(format!(
                "MCP server returned status {status}: {}",
                String::from_utf8_lossy(&body)
            ));
        }
        Ok(())
    }

    async fn send(&self, payload: &Value) -> Result<(u16, hyper::HeaderMap, Bytes), String> {
        let body = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
        let mut builder = Request::builder()
            .method("POST")
            .uri(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL_VERSION);
        if let Some(session) = &self.session_id {
            builder = builder.header("Mcp-Session-Id", session);
        }
        for (key, value) in &self.headers {
            builder = builder.header(key.as_str(), value.as_str());
        }
        let req = builder
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| e.to_string())?;

        tinynet::send_request_with_headers(req)
            .await
            .map_err(|e| e.to_string())
    }
}

fn parse_response(headers: &hyper::HeaderMap, body: &[u8]) -> Result<Value, String> {
    let is_sse =
        header_str(headers, "content-type").is_some_and(|ct| ct.contains("text/event-stream"));
    if is_sse {
        parse_sse(body)
    } else {
        serde_json::from_slice(body).map_err(|e| e.to_string())
    }
}

/// Extract the single JSON-RPC response message from an SSE body.
fn parse_sse(body: &[u8]) -> Result<Value, String> {
    let mut decoder = tinynet::SseDecoder::new();
    let mut found = None;
    let mut buf = body.to_vec();
    buf.extend_from_slice(b"\n\n");
    let _ = decoder.push::<(), _>(&buf, |data| {
        if let Ok(value) = serde_json::from_slice::<Value>(data)
            && (value.get("result").is_some() || value.get("error").is_some())
        {
            found = Some(value);
        }
        Ok(())
    });
    found.ok_or_else(|| "no JSON-RPC response in MCP SSE stream".to_string())
}

/// Read the MCP `annotations.readOnlyHint`. Per the spec it defaults to false, so
/// a tool is considered read-only only when the server explicitly sets it; any
/// other tool is treated as state-changing and gated behind approval.
fn read_only_hint(tool: &Value) -> bool {
    tool.get("annotations")
        .and_then(|annotations| annotations.get("readOnlyHint"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn header_str(headers: &hyper::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|s| s.to_string())
}

/// Convert an MCP tool `inputSchema` into the llm crate's parameter shape,
/// passing through unmodelled JSON Schema keywords via `extra`.
fn parameters_from_schema(schema: Value) -> Option<FunctionParameters> {
    let Value::Object(mut obj) = schema else {
        return None;
    };
    let param_type = take_string(&mut obj, "type").unwrap_or_else(|| "object".to_string());
    let required = obj
        .remove("required")
        .and_then(|value| serde_json::from_value(value).ok());
    let properties = match obj.remove("properties") {
        Some(Value::Object(props)) => props
            .into_iter()
            .map(|(name, value)| (name, property_from_schema(value)))
            .collect(),
        _ => Default::default(),
    };

    Some(FunctionParameters {
        param_type,
        properties,
        required,
        extra: obj,
    })
}

fn property_from_schema(schema: Value) -> FunctionProperty {
    let Value::Object(mut obj) = schema else {
        return FunctionProperty::default();
    };
    FunctionProperty {
        r#type: take_string(&mut obj, "type").unwrap_or_default(),
        description: take_string(&mut obj, "description").unwrap_or_default(),
        extra: obj,
    }
}

/// Remove `key` from `obj`, returning it only when it is a string.
fn take_string(obj: &mut Map<String, Value>, key: &str) -> Option<String> {
    match obj.remove(key)? {
        Value::String(s) => Some(s),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_maps_properties_and_required() {
        let schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string", "description": "City name" },
                "units": { "type": "string", "enum": ["c", "f"] }
            },
            "required": ["city"],
            "additionalProperties": false
        });

        let params = parameters_from_schema(schema).unwrap();
        assert_eq!(params.param_type, "object");
        assert_eq!(params.required, Some(vec!["city".to_string()]));

        let city = &params.properties["city"];
        assert_eq!(city.r#type, "string");
        assert_eq!(city.description, "City name");

        let units = &params.properties["units"];
        assert_eq!(units.extra["enum"], json!(["c", "f"]));

        // Unmodelled top-level keywords are preserved verbatim.
        assert_eq!(params.extra["additionalProperties"], json!(false));
    }

    #[test]
    fn non_object_schema_is_ignored() {
        assert!(parameters_from_schema(json!("nonsense")).is_none());
    }

    #[test]
    fn read_only_hint_defaults_to_false_and_honors_annotation() {
        assert!(!read_only_hint(&json!({ "name": "create_issue" })));
        assert!(!read_only_hint(
            &json!({ "annotations": { "readOnlyHint": false } })
        ));
        assert!(read_only_hint(
            &json!({ "annotations": { "readOnlyHint": true } })
        ));
    }

    #[test]
    fn missing_type_defaults_to_object_without_duplicate_keys() {
        let params = parameters_from_schema(json!({ "properties": {} })).unwrap();
        assert_eq!(params.param_type, "object");
        let serialized = serde_json::to_value(&params).unwrap();
        assert_eq!(serialized["type"], json!("object"));
    }
}
