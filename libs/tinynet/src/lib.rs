use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures::Stream;
use http_body_util::BodyExt;
use hyper::Request;
use hyper::body::Body;
use hyper::header::{HeaderName, HeaderValue};
use thiserror::Error;
use tinynet_core::client::{Client, ClientConfig};
use tinynet_core::http::{Method, Request as CoreRequest, Response as CoreResponse};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Request error: {0}")]
    RequestError(String),
    #[error("TLS error: {0}")]
    TlsError(String),
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    #[error("Server error {0}: {1}")]
    ServerError(u16, String),
}

const DEFAULT_TIMEOUT_SECS: u64 = 20;
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
const MAX_IDLE_PER_KEY: usize = 8;
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);

fn default_timeout() -> Duration {
    timeout_from_env("TINYNET_TIMEOUT_SECS", DEFAULT_TIMEOUT_SECS)
}

fn connect_timeout() -> Duration {
    timeout_from_env("TINYNET_CONNECT_TIMEOUT_SECS", DEFAULT_CONNECT_TIMEOUT_SECS)
}

fn timeout_from_env(name: &str, fallback: u64) -> Duration {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(fallback))
}

#[derive(Clone, Eq)]
struct ClientKey {
    scheme: String,
    host: String,
    request_timeout: Option<Duration>,
    connect_timeout: Duration,
}

impl PartialEq for ClientKey {
    fn eq(&self, other: &Self) -> bool {
        self.scheme == other.scheme
            && self.host == other.host
            && self.request_timeout == other.request_timeout
            && self.connect_timeout == other.connect_timeout
    }
}

impl Hash for ClientKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.scheme.hash(state);
        self.host.hash(state);
        self.request_timeout.hash(state);
        self.connect_timeout.hash(state);
    }
}

struct IdleClient {
    client: Client,
    since: Instant,
}

static CLIENTS: OnceLock<Mutex<HashMap<ClientKey, Vec<IdleClient>>>> = OnceLock::new();

fn clients() -> &'static Mutex<HashMap<ClientKey, Vec<IdleClient>>> {
    CLIENTS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn checkout_client(key: &ClientKey) -> Result<(Client, bool), Error> {
    if let Some(idle) = clients().lock().unwrap().get_mut(key) {
        idle.retain(|client| client.since.elapsed() < IDLE_TIMEOUT);
        if let Some(client) = idle.pop() {
            return Ok((client.client, true));
        }
    }

    Ok((new_client(key).await?, false))
}

async fn new_client(key: &ClientKey) -> Result<Client, Error> {
    let config = ClientConfig {
        follow_redirects: false,
        protocol_upgrades: false,
        connect_timeout: Some(key.connect_timeout),
        request_timeout: key.request_timeout,
        ..ClientConfig::default()
    };
    Client::from_parts_with_config(&key.scheme, &key.host, config)
        .await
        .map_err(request_error)
}

fn checkin_client(key: ClientKey, client: Client) {
    let mut clients = clients().lock().unwrap();
    let idle = clients.entry(key).or_default();
    if idle.len() < MAX_IDLE_PER_KEY {
        idle.push(IdleClient {
            client,
            since: Instant::now(),
        });
    }
}

pub async fn send_request<T>(request: Request<T>) -> Result<(u16, Bytes), Error>
where
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    send_request_with_timeout(request, default_timeout()).await
}

pub async fn send_request_with_timeout<T>(
    request: Request<T>,
    timeout: Duration,
) -> Result<(u16, Bytes), Error>
where
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let (status, _, body) = send_request_with_headers_timeout(request, timeout).await?;
    Ok((status, body))
}

pub async fn send_request_with_headers<T>(
    request: Request<T>,
) -> Result<(u16, hyper::HeaderMap, Bytes), Error>
where
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    send_request_with_headers_timeout(request, default_timeout()).await
}

pub async fn send_request_with_headers_timeout<T>(
    request: Request<T>,
    timeout: Duration,
) -> Result<(u16, hyper::HeaderMap, Bytes), Error>
where
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let (request, key) = convert_request(request, Some(timeout)).await?;
    let (mut client, reused) = checkout_client(&key).await?;
    let response = match client.send(&request).await {
        Ok(response) => response,
        Err(_) if reused => {
            client = new_client(&key).await?;
            client.send(&request).await.map_err(request_error)?
        }
        Err(error) => return Err(request_error(error)),
    };
    let response = convert_response(response)?;
    checkin_client(key, client);
    Ok(response)
}

pub async fn stream_request<T>(
    request: Request<T>,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, Error>> + Send + 'static>>
where
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let (request, key) = match convert_request(request, None).await {
        Ok(converted) => converted,
        Err(error) => return Box::pin(futures::stream::once(async move { Err(error) })),
    };
    let mut client = match checkout_client(&key).await {
        Ok((client, _)) => client,
        Err(error) => return Box::pin(futures::stream::once(async move { Err(error) })),
    };

    let stream = async_stream::stream! {
        let response = match client.send_streaming(&request).await {
            Ok(response) => response,
            Err(error) => {
                yield Err(request_error(error));
                return;
            }
        };
        let tinynet_core::http::body::StreamingResponse { head, body } = response;
        let status = head.status;
        if !(200..300).contains(&status) {
            match body.collect().await {
                Ok(body) => {
                    let message = String::from_utf8_lossy(body.as_ref()).into_owned();
                    yield Err(Error::ServerError(status, message));
                }
                Err(error) => yield Err(request_error(error)),
            }
            return;
        }

        let mut body = body;
        loop {
            match body.chunk().await {
                Ok(Some(chunk)) => yield Ok(Bytes::copy_from_slice(chunk.as_ref())),
                Ok(None) => break,
                Err(error) => {
                    yield Err(request_error(error));
                    return;
                }
            }
        }
        drop(body);
    };
    Box::pin(stream)
}

async fn convert_request<T>(
    request: Request<T>,
    request_timeout: Option<Duration>,
) -> Result<(CoreRequest<'static>, ClientKey), Error>
where
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let (parts, body) = request.into_parts();
    let scheme = parts
        .uri
        .scheme_str()
        .ok_or_else(|| Error::InvalidRequest("missing URL scheme".into()))?
        .to_owned();
    let host = parts
        .uri
        .authority()
        .ok_or_else(|| Error::InvalidRequest("missing host".into()))?
        .as_str()
        .to_owned();
    let target = parts
        .uri
        .path_and_query()
        .map_or("/", |value| value.as_str())
        .to_owned();
    let method = convert_method(parts.method);
    let headers = parts
        .headers
        .iter()
        .map(|(name, value)| {
            Ok((
                name.as_str().to_owned().into(),
                value
                    .to_str()
                    .map_err(|error| Error::InvalidRequest(error.to_string()))?
                    .to_owned()
                    .into(),
            ))
        })
        .collect::<Result<_, Error>>()?;
    let body = body
        .collect()
        .await
        .map_err(|error| {
            let error: Box<dyn std::error::Error + Send + Sync> = error.into();
            Error::RequestError(error.to_string())
        })?
        .to_bytes();

    let key = ClientKey {
        scheme: scheme.clone(),
        host: host.clone(),
        request_timeout,
        connect_timeout: connect_timeout(),
    };
    Ok((
        CoreRequest {
            method,
            scheme: scheme.into(),
            host: host.into(),
            target: target.into(),
            headers,
            body: (!body.is_empty()).then(|| body.to_vec().into()),
        },
        key,
    ))
}

fn convert_method(method: hyper::Method) -> Method<'static> {
    match method {
        hyper::Method::GET => Method::GET,
        hyper::Method::POST => Method::POST,
        hyper::Method::PUT => Method::PUT,
        hyper::Method::DELETE => Method::DELETE,
        hyper::Method::PATCH => Method::PATCH,
        hyper::Method::HEAD => Method::HEAD,
        hyper::Method::OPTIONS => Method::OPTIONS,
        hyper::Method::CONNECT => Method::CONNECT,
        hyper::Method::TRACE => Method::TRACE,
        method => Method::Custom(method.as_str().to_owned().into()),
    }
}

fn convert_response(
    response: CoreResponse<'static>,
) -> Result<(u16, hyper::HeaderMap, Bytes), Error> {
    let mut headers = hyper::HeaderMap::new();
    for (name, value) in response.headers {
        let name = HeaderName::from_bytes(name.as_ref().as_bytes())
            .map_err(|error| Error::RequestError(error.to_string()))?;
        let value = HeaderValue::from_bytes(value.as_ref().as_bytes())
            .map_err(|error| Error::RequestError(error.to_string()))?;
        headers.append(name, value);
    }
    let body = response
        .body
        .map_or_else(Bytes::new, |body| Bytes::copy_from_slice(body.as_ref()));
    Ok((response.status, headers, body))
}

fn request_error(error: std::io::Error) -> Error {
    Error::RequestError(error.to_string())
}

#[derive(Default)]
pub struct SseDecoder(tinynet_core::codec::sse::SseDecoder);

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push<E, F>(&mut self, chunk: &[u8], mut on_data: F) -> Result<bool, E>
    where
        F: FnMut(&[u8]) -> Result<(), E>,
    {
        self.0.push(chunk);
        for event in self.0.by_ref() {
            if event.data == "[DONE]" {
                return Ok(true);
            }
            on_data(event.data.as_bytes())?;
        }
        Ok(false)
    }
}

#[derive(Default)]
pub struct NdjsonDecoder(tinynet_core::codec::ndjson::NdjsonDecoder);

impl NdjsonDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push<E, F>(&mut self, chunk: &[u8], mut on_line: F) -> Result<(), E>
    where
        F: FnMut(&[u8]) -> Result<(), E>,
    {
        self.0.push(chunk);
        for line in self.0.by_ref() {
            let line = trim_ascii(&line);
            if !line.is_empty() {
                on_line(line)?;
            }
        }
        Ok(())
    }
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}
