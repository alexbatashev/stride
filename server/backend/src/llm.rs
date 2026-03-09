use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chacha20poly1305::AeadCore;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use friday::grpc::generated::friday::core::rpc::language_model_server::{
    LanguageModel, LanguageModelServer,
};
use friday::grpc::generated::friday::core::rpc::{
    CompletionChunk, CompletionRequest, Empty, FunctionRef, Message, MessageContentType,
    MessageRole, ModelDesc, ModelList, ProviderKind, RegisterModelRequest, RegisterProviderRequest,
    Tool as ProtoTool, ToolCallChunk, ToolCallFunctionChunk, ToolChoice, ToolType,
    UnnamedToolChoice, tool_choice,
};
use futures::{Stream, StreamExt};
use jsonwebtoken::{DecodingKey, Validation, decode};
use llm::{
    API, Anthropic, CompletionChoice, OpenAI, Role, Tool as LlmTool,
    UnnamedToolChoice as LlmUnnamedToolChoice,
};
use minisql::{ConnectionPool, Value};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tonic::metadata::MetadataMap;
use tonic::{Request, Response, Status, async_trait};
use uuid::Uuid;

use crate::{llm_models, llm_providers, server_sessions};

const PROVIDER_KEY_ENV: &str = "FRIDAY_PROVIDER_ENCRYPTION_KEY";

#[derive(Clone)]
struct ProviderTokenCrypto {
    key: [u8; 32],
}

impl ProviderTokenCrypto {
    fn from_env_or_jwt(jwt_secret: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if let Ok(raw_key) = std::env::var(PROVIDER_KEY_ENV) {
            let key = parse_provider_key(&raw_key)?;
            return Ok(Self { key });
        }

        eprintln!(
            "WARN: {PROVIDER_KEY_ENV} is not set, deriving provider encryption key from FRIDAY_JWT_SECRET"
        );
        let mut hasher = Sha256::new();
        hasher.update(jwt_secret.as_bytes());
        hasher.update(b":friday-provider-key-v1");
        let digest = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&digest);
        Ok(Self { key })
    }

    fn encrypt(&self, plaintext: &str) -> Result<(String, String), Status> {
        let cipher = XChaCha20Poly1305::new((&self.key).into());
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|_| Status::internal("failed to encrypt provider token"))?;
        Ok((
            BASE64.encode(nonce.as_slice()),
            BASE64.encode(ciphertext.as_slice()),
        ))
    }

    fn decrypt(&self, nonce_b64: &str, ciphertext_b64: &str) -> Result<String, Status> {
        let nonce = BASE64
            .decode(nonce_b64)
            .map_err(|_| Status::internal("failed to decode provider token nonce"))?;
        if nonce.len() != 24 {
            return Err(Status::internal("invalid provider token nonce length"));
        }
        let ciphertext = BASE64
            .decode(ciphertext_b64)
            .map_err(|_| Status::internal("failed to decode provider token ciphertext"))?;
        let cipher = XChaCha20Poly1305::new((&self.key).into());
        let plaintext = cipher
            .decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
            .map_err(|_| Status::internal("failed to decrypt provider token"))?;
        String::from_utf8(plaintext)
            .map_err(|_| Status::internal("provider token has invalid utf-8 content"))
    }
}

fn parse_provider_key(raw_key: &str) -> Result<[u8; 32], Box<dyn std::error::Error + Send + Sync>> {
    let trimmed = raw_key.trim();
    if trimmed.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{PROVIDER_KEY_ENV} must not be empty"),
        )
        .into());
    }

    if trimmed.as_bytes().len() == 32 {
        let mut key = [0u8; 32];
        key.copy_from_slice(trimmed.as_bytes());
        return Ok(key);
    }

    let decoded = BASE64.decode(trimmed).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{PROVIDER_KEY_ENV} must be 32 raw bytes or base64-encoded 32-byte key"),
        )
    })?;
    if decoded.len() != 32 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "{PROVIDER_KEY_ENV} must decode to exactly 32 bytes, got {}",
                decoded.len()
            ),
        )
        .into());
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&decoded);
    Ok(key)
}

#[derive(Clone)]
pub struct LanguageModelService {
    db: Arc<ConnectionPool>,
    crypto: Arc<ProviderTokenCrypto>,
    jwt_secret: Arc<String>,
}

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    sid: String,
    exp: usize,
    iat: usize,
}

type LlmStream = Pin<Box<dyn Stream<Item = Result<CompletionChunk, Status>> + Send + 'static>>;

#[async_trait]
impl LanguageModel for LanguageModelService {
    type CompleteStream = LlmStream;

    async fn register_provider(
        &self,
        request: Request<RegisterProviderRequest>,
    ) -> Result<Response<Empty>, Status> {
        let user_id = authenticate(&self.db, &self.jwt_secret, request.metadata()).await?;
        let body = request.into_inner();
        let provider_id = validate_provider_request(&body)?;
        let (token_nonce, token_ciphertext) = self.crypto.encrypt(body.token.trim())?;
        let now = now_epoch_seconds();

        let existing = llm_providers::select()
            .where_(llm_providers::id.eq(provider_id))
            .limit(1)
            .all(&self.db)
            .await
            .map_err(|_| Status::internal("failed to query providers"))?;

        if let Some(existing) = existing.first() {
            if existing.user_id != user_id {
                return Err(Status::permission_denied(
                    "provider belongs to another user",
                ));
            }
            self.db
                .query_with_params(
                    "UPDATE llm_providers SET provider_name = ?, kind = ?, api_base_url = ?, token_nonce = ?, token_ciphertext = ?, updated_at = ? WHERE id = ?",
                    vec![
                        Value::Text(body.provider_name.trim().to_owned()),
                        Value::Integer(body.kind as i64),
                        Value::Text(body.api_base_url.trim().to_owned()),
                        Value::Text(token_nonce),
                        Value::Text(token_ciphertext),
                        Value::Integer(now),
                        Value::Uuid(provider_id),
                    ],
                )
                .await
                .map_err(|_| Status::internal("failed to update provider"))?;
            return Ok(Response::new(Empty {}));
        }

        self.db
            .query_with_params(
                "INSERT INTO llm_providers (id, user_id, provider_name, kind, api_base_url, token_nonce, token_ciphertext, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(provider_id),
                    Value::Uuid(user_id),
                    Value::Text(body.provider_name.trim().to_owned()),
                    Value::Integer(body.kind as i64),
                    Value::Text(body.api_base_url.trim().to_owned()),
                    Value::Text(token_nonce),
                    Value::Text(token_ciphertext),
                    Value::Integer(now),
                    Value::Integer(now),
                ],
            )
            .await
            .map_err(|_| Status::internal("failed to store provider"))?;

        Ok(Response::new(Empty {}))
    }

    async fn register_model(
        &self,
        request: Request<RegisterModelRequest>,
    ) -> Result<Response<Empty>, Status> {
        let user_id = authenticate(&self.db, &self.jwt_secret, request.metadata()).await?;
        let body = request.into_inner();
        let provider_id = validate_model_request(&body)?;

        let provider_exists = llm_providers::select()
            .where_(llm_providers::id.eq(provider_id))
            .where_(llm_providers::user_id.eq(user_id))
            .limit(1)
            .all(&self.db)
            .await
            .map_err(|_| Status::internal("failed to query providers"))?;
        if provider_exists.is_empty() {
            return Err(Status::not_found("provider does not exist"));
        }

        let model_slug = body.model_slug.trim().to_owned();
        let now = now_epoch_seconds();

        let existing_model = llm_models::select()
            .where_(llm_models::user_id.eq(user_id))
            .where_(llm_models::provider_id.eq(provider_id))
            .where_(llm_models::model_slug.eq(model_slug.as_str()))
            .limit(1)
            .all(&self.db)
            .await
            .map_err(|_| Status::internal("failed to query models"))?;

        if let Some(existing_model) = existing_model.first() {
            self.db
                .query_with_params(
                    "UPDATE llm_models SET model_name = ?, supports_thinking = ?, supports_tools = ?, supports_image_input = ?, updated_at = ? WHERE id = ?",
                    vec![
                        Value::Text(body.model_name.trim().to_owned()),
                        Value::Integer(if body.supports_thinking { 1 } else { 0 }),
                        Value::Integer(if body.supports_tools { 1 } else { 0 }),
                        Value::Integer(if body.supports_image_input { 1 } else { 0 }),
                        Value::Integer(now),
                        Value::Uuid(existing_model.id),
                    ],
                )
                .await
                .map_err(|_| Status::internal("failed to update model"))?;
            return Ok(Response::new(Empty {}));
        }

        self.db
            .query_with_params(
                "INSERT INTO llm_models (id, user_id, provider_id, model_slug, model_name, supports_thinking, supports_tools, supports_image_input, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(Uuid::new_v4()),
                    Value::Uuid(user_id),
                    Value::Uuid(provider_id),
                    Value::Text(model_slug),
                    Value::Text(body.model_name.trim().to_owned()),
                    Value::Integer(if body.supports_thinking { 1 } else { 0 }),
                    Value::Integer(if body.supports_tools { 1 } else { 0 }),
                    Value::Integer(if body.supports_image_input { 1 } else { 0 }),
                    Value::Integer(now),
                    Value::Integer(now),
                ],
            )
            .await
            .map_err(|_| Status::internal("failed to store model"))?;

        Ok(Response::new(Empty {}))
    }

    async fn list(&self, request: Request<Empty>) -> Result<Response<ModelList>, Status> {
        let user_id = authenticate(&self.db, &self.jwt_secret, request.metadata()).await?;
        let providers = llm_providers::select()
            .where_(llm_providers::user_id.eq(user_id))
            .all(&self.db)
            .await
            .map_err(|_| Status::internal("failed to query providers"))?;
        let models = llm_models::select()
            .where_(llm_models::user_id.eq(user_id))
            .all(&self.db)
            .await
            .map_err(|_| Status::internal("failed to query models"))?;

        let provider_names: HashMap<_, _> = providers
            .into_iter()
            .map(|provider| (provider.id, provider.provider_name))
            .collect();

        let mut out = models
            .into_iter()
            .map(|model| ModelDesc {
                provider_id: model.provider_id.to_string(),
                provider_name: provider_names
                    .get(&model.provider_id)
                    .cloned()
                    .unwrap_or_else(|| model.provider_id.to_string()),
                model_slug: model.model_slug,
                model_name: model.model_name,
                model_desc: String::new(),
                supports_thinking: model.supports_thinking,
                supports_tools: model.supports_tools,
                supports_image_input: model.supports_image_input,
            })
            .collect::<Vec<_>>();
        out.sort_by(|a, b| {
            a.provider_id
                .cmp(&b.provider_id)
                .then_with(|| a.model_slug.cmp(&b.model_slug))
        });

        Ok(Response::new(ModelList { models: out }))
    }

    async fn complete(
        &self,
        request: Request<CompletionRequest>,
    ) -> Result<Response<Self::CompleteStream>, Status> {
        let user_id = authenticate(&self.db, &self.jwt_secret, request.metadata()).await?;
        let body = request.into_inner();
        let provider_id = parse_provider_uuid(&body.provider_id)?;
        if body.model_id.trim().is_empty() {
            return Err(Status::invalid_argument("modelId is required"));
        }

        let providers = llm_providers::select()
            .where_(llm_providers::id.eq(provider_id))
            .where_(llm_providers::user_id.eq(user_id))
            .limit(1)
            .all(&self.db)
            .await
            .map_err(|_| Status::internal("failed to query provider"))?;
        let Some(provider) = providers.into_iter().next() else {
            return Err(Status::not_found("provider does not exist"));
        };

        let models = llm_models::select()
            .where_(llm_models::user_id.eq(user_id))
            .where_(llm_models::provider_id.eq(provider_id))
            .where_(llm_models::model_slug.eq(body.model_id.trim()))
            .limit(1)
            .all(&self.db)
            .await
            .map_err(|_| Status::internal("failed to query model"))?;
        let Some(model) = models.into_iter().next() else {
            return Err(Status::not_found("model does not exist for provider"));
        };

        let token = self
            .crypto
            .decrypt(&provider.token_nonce, &provider.token_ciphertext)?;
        let api = provider_api(provider.kind as i32, &provider.api_base_url)?;
        let completion_request = proto_completion_to_llm(&body, &model.model_slug)?;

        let upstream = api.stream_completion(&token, completion_request);
        let stream = async_stream::try_stream! {
            futures::pin_mut!(upstream);
            while let Some(item) = upstream.next().await {
                let chunk = match item {
                    Ok(chunk) => chunk,
                    Err(error) => Err(map_llm_error(error))?,
                };
                for choice in chunk.choices {
                    yield completion_choice_to_proto(choice);
                }
            }
        };

        Ok(Response::new(Box::pin(stream) as Self::CompleteStream))
    }
}

pub(crate) fn language_model_service(
    db: Arc<ConnectionPool>,
    jwt_secret: &str,
) -> Result<LanguageModelServer<LanguageModelService>, Box<dyn std::error::Error + Send + Sync>> {
    let crypto = ProviderTokenCrypto::from_env_or_jwt(jwt_secret)?;
    Ok(LanguageModelServer::new(LanguageModelService {
        db,
        crypto: Arc::new(crypto),
        jwt_secret: Arc::new(jwt_secret.to_owned()),
    }))
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn validate_provider_request(body: &RegisterProviderRequest) -> Result<Uuid, Status> {
    let provider_id = parse_provider_uuid(&body.provider_id)?;
    if body.provider_name.trim().is_empty() {
        return Err(Status::invalid_argument("providerName is required"));
    }
    if body.api_base_url.trim().is_empty() {
        return Err(Status::invalid_argument("apiBaseUrl is required"));
    }
    if body.token.trim().is_empty() {
        return Err(Status::invalid_argument("token is required"));
    }
    if ProviderKind::try_from(body.kind).unwrap_or(ProviderKind::Unspecified)
        == ProviderKind::Unspecified
    {
        return Err(Status::invalid_argument("provider kind must be specified"));
    }
    Ok(provider_id)
}

fn validate_model_request(body: &RegisterModelRequest) -> Result<Uuid, Status> {
    let provider_id = parse_provider_uuid(&body.provider_id)?;
    if body.model_slug.trim().is_empty() {
        return Err(Status::invalid_argument("modelSlug is required"));
    }
    if body.model_name.trim().is_empty() {
        return Err(Status::invalid_argument("modelName is required"));
    }
    Ok(provider_id)
}

fn parse_provider_uuid(provider_id: &str) -> Result<Uuid, Status> {
    let provider_id = provider_id.trim();
    if provider_id.is_empty() {
        return Err(Status::invalid_argument("providerId is required"));
    }
    Uuid::parse_str(provider_id)
        .map_err(|_| Status::invalid_argument("providerId must be a valid UUID"))
}

fn bearer_token(metadata: &MetadataMap) -> Result<&str, Status> {
    let raw = metadata
        .get("authorization")
        .ok_or_else(|| Status::unauthenticated("missing authorization metadata"))?;
    let auth = raw
        .to_str()
        .map_err(|_| Status::unauthenticated("invalid authorization metadata"))?;
    let token = auth
        .strip_prefix("Bearer ")
        .ok_or_else(|| Status::unauthenticated("authorization must be Bearer token"))?;
    if token.is_empty() {
        return Err(Status::unauthenticated("empty bearer token"));
    }
    Ok(token)
}

fn cookie_token(metadata: &MetadataMap) -> Option<String> {
    let raw = metadata.get("cookie")?;
    let cookie_header = raw.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("friday_auth=") {
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

fn request_token(metadata: &MetadataMap) -> Result<String, Status> {
    if let Ok(token) = bearer_token(metadata) {
        return Ok(token.to_owned());
    }
    if let Some(token) = cookie_token(metadata) {
        return Ok(token);
    }
    Err(Status::unauthenticated(
        "missing auth token (expected Bearer metadata or friday_auth cookie)",
    ))
}

fn decode_jwt(jwt_secret: &str, token: &str) -> Result<Claims, Status> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| Status::unauthenticated("invalid token"))?;
    Ok(token_data.claims)
}

async fn authenticate(
    db: &ConnectionPool,
    jwt_secret: &str,
    metadata: &MetadataMap,
) -> Result<Uuid, Status> {
    let token = request_token(metadata)?;
    let claims = decode_jwt(jwt_secret, &token)?;
    let sid = Uuid::parse_str(&claims.sid).map_err(|_| Status::unauthenticated("invalid token"))?;
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| Status::unauthenticated("invalid token"))?;

    let rows = server_sessions::select()
        .where_(server_sessions::token_id.eq(sid))
        .limit(1)
        .all(db)
        .await
        .map_err(|_| Status::internal("failed to load session"))?;

    let row = rows
        .first()
        .ok_or_else(|| Status::unauthenticated("unknown session"))?;

    if row.user_id != user_id {
        return Err(Status::unauthenticated("invalid session user"));
    }
    if row.revoked_at.is_some() {
        return Err(Status::unauthenticated("session revoked"));
    }
    if row.expires_at <= now_epoch_seconds() {
        return Err(Status::unauthenticated("session expired"));
    }

    Ok(user_id)
}

fn provider_api(kind: i32, api_base_url: &str) -> Result<API, Status> {
    let kind = ProviderKind::try_from(kind).unwrap_or(ProviderKind::Unspecified);
    match kind {
        ProviderKind::OpenaiLike => Ok(OpenAI::new(api_base_url.trim())),
        ProviderKind::Anthropic => Ok(Anthropic::new(api_base_url.trim())),
        ProviderKind::Unspecified => Err(Status::invalid_argument("unsupported provider kind")),
    }
}

fn map_llm_error(error: llm::Error) -> Status {
    match error {
        llm::Error::InvalidRequest(message) => Status::invalid_argument(message),
        llm::Error::ServerError(code) => {
            Status::unavailable(format!("upstream provider returned status {code}"))
        }
        llm::Error::TlsError(message) => Status::unavailable(format!("tls error: {message}")),
        llm::Error::RequestError(message) => Status::unavailable(message),
        llm::Error::ParsingError(message) => Status::internal(message),
        llm::Error::Unknown => Status::unknown("unknown llm error"),
    }
}

fn proto_completion_to_llm(
    req: &CompletionRequest,
    model_slug: &str,
) -> Result<llm::CompletionRequest, Status> {
    let messages = req
        .messages
        .iter()
        .map(proto_message_to_llm)
        .collect::<Result<Vec<_>, _>>()?;
    let mut request = llm::CompletionRequest::new(model_slug, &messages);

    if !req.tools.is_empty() {
        let tools = req
            .tools
            .iter()
            .map(proto_tool_to_llm)
            .collect::<Result<Vec<_>, _>>()?;
        request = request.tools(tools);
    }

    if let Some(choice) = req.tool_choice.as_ref() {
        request = request.tool_choice(proto_tool_choice_to_llm(choice)?);
    }

    Ok(request)
}

fn proto_message_to_llm(message: &Message) -> Result<llm::Message, Status> {
    let role = match MessageRole::try_from(message.role).unwrap_or(MessageRole::Unspecified) {
        MessageRole::System => Role::System,
        MessageRole::Assistant => Role::Assistant,
        MessageRole::User => Role::User,
        MessageRole::Tool => Role::Tool,
        MessageRole::Unspecified => {
            return Err(Status::invalid_argument("message role must be specified"));
        }
    };

    let content = flatten_content(&message.content);
    let thinking = if message.thinking {
        Some(content.clone())
    } else {
        None
    };

    Ok(llm::Message {
        role,
        content,
        thinking,
        tool_call_id: (!message.tool_call_id.trim().is_empty())
            .then(|| message.tool_call_id.clone()),
    })
}

fn flatten_content(
    content: &[friday::grpc::generated::friday::core::rpc::MessageContent],
) -> String {
    content
        .iter()
        .map(|part| {
            let ty =
                MessageContentType::try_from(part.ty).unwrap_or(MessageContentType::Unspecified);
            match ty {
                MessageContentType::Text | MessageContentType::Unspecified => part.data.clone(),
                MessageContentType::ImageUrl => {
                    format!("[image_url] {}", part.data)
                }
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn proto_tool_to_llm(tool: &ProtoTool) -> Result<LlmTool, Status> {
    let ty = ToolType::try_from(tool.r#type).unwrap_or(ToolType::Unspecified);
    if ty != ToolType::Function {
        return Err(Status::invalid_argument(
            "only function tools are supported",
        ));
    }
    let function = tool
        .function
        .as_ref()
        .ok_or_else(|| Status::invalid_argument("tool function is required"))?;
    if function.name.trim().is_empty() {
        return Err(Status::invalid_argument("tool function name is required"));
    }

    let parameters = if function.parameters.is_empty() {
        None
    } else {
        Some(
            function
                .parameters
                .iter()
                .map(|param| llm::FunctionParameters {
                    r#type: param.r#type.clone(),
                    properties: param
                        .properties
                        .iter()
                        .map(|(k, v)| {
                            (
                                k.clone(),
                                llm::FunctionProperty {
                                    r#type: v.r#type.clone(),
                                    description: v.description.clone(),
                                },
                            )
                        })
                        .collect(),
                    required: (!param.required.is_empty()).then(|| param.required.clone()),
                })
                .collect(),
        )
    };

    Ok(LlmTool {
        r#type: llm::ToolType::Function,
        function: llm::Function {
            description: function.description.clone(),
            name: function.name.clone(),
            parameters,
        },
    })
}

fn proto_tool_choice_to_llm(choice: &ToolChoice) -> Result<llm::ToolChoice, Status> {
    match choice.choice.as_ref() {
        Some(tool_choice::Choice::Unnamed(choice)) => {
            let unnamed = match UnnamedToolChoice::try_from(*choice)
                .unwrap_or(UnnamedToolChoice::Unspecified)
            {
                UnnamedToolChoice::None => LlmUnnamedToolChoice::None,
                UnnamedToolChoice::Auto => LlmUnnamedToolChoice::Auto,
                UnnamedToolChoice::Required => LlmUnnamedToolChoice::Required,
                UnnamedToolChoice::Unspecified => {
                    return Err(Status::invalid_argument(
                        "toolChoice.unnamed must be specified",
                    ));
                }
            };
            Ok(llm::ToolChoice::Unnamed(unnamed))
        }
        Some(tool_choice::Choice::NamedFunction(FunctionRef { name })) => {
            if name.trim().is_empty() {
                return Err(Status::invalid_argument(
                    "toolChoice.namedFunction.name must be set",
                ));
            }
            Ok(llm::ToolChoice::Named {
                r#type: "function".to_owned(),
                function: llm::FunctionRef::new(name.clone()),
            })
        }
        None => Err(Status::invalid_argument("toolChoice must be set")),
    }
}

fn completion_choice_to_proto(choice: CompletionChoice) -> CompletionChunk {
    let content = choice
        .delta
        .as_ref()
        .and_then(|delta| delta.content.clone())
        .or_else(|| choice.text.clone())
        .or_else(|| {
            choice
                .message
                .as_ref()
                .map(|message| message.content.clone())
        })
        .unwrap_or_default();

    let thinking = choice
        .delta
        .as_ref()
        .and_then(|delta| delta.thinking.clone())
        .or_else(|| choice.message.and_then(|message| message.thinking))
        .unwrap_or_default();

    let tool_calls = choice
        .delta
        .as_ref()
        .and_then(|delta| delta.tool_calls.clone())
        .or(choice.tool_calls)
        .unwrap_or_default()
        .into_iter()
        .map(|tool_call| ToolCallChunk {
            index: tool_call.index.unwrap_or(0) as u32,
            id: tool_call.id.unwrap_or_default(),
            function: tool_call.function.map(|function| ToolCallFunctionChunk {
                name: function.name.unwrap_or_default(),
                arguments: function.arguments.unwrap_or_default(),
            }),
        })
        .collect::<Vec<_>>();

    CompletionChunk {
        choice_index: choice.index as u32,
        content,
        thinking,
        tool_calls,
        finish_reason: choice.finish_reason.unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::{PROVIDER_KEY_ENV, ProviderTokenCrypto, parse_provider_key};

    #[test]
    fn parse_raw_32_byte_key() {
        let key = parse_provider_key("0123456789abcdef0123456789abcdef").expect("parse");
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn parse_key_rejects_invalid_length() {
        let err = parse_provider_key("short-key").expect_err("must fail");
        assert!(err.to_string().contains(PROVIDER_KEY_ENV));
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let crypto = ProviderTokenCrypto {
            key: *b"0123456789abcdef0123456789abcdef",
        };
        let token = "sk-test-123";
        let (nonce, ciphertext) = crypto.encrypt(token).expect("encrypt");
        assert_ne!(ciphertext, token);

        let decrypted = crypto.decrypt(&nonce, &ciphertext).expect("decrypt");
        assert_eq!(decrypted, token);
    }
}
