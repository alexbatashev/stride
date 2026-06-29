mod stream;

use std::net::{SocketAddr, ToSocketAddrs};
use std::pin::Pin;
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures::FutureExt;
use futures::Stream;
use futures::future::Either;
use http_body_util::BodyExt;
use hyper::Request;
use hyper::body::Body;
use hyper::header::{HOST, HeaderValue};
use thiserror::Error;

use stream::{AsyncTcpStream, AsyncTlsStream};

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

/// Default overall request deadline (connect + handshake + transfer). Without
/// this, a stalled host makes the busy-polling stream spin forever, which hung
/// `web_search` academic lookups indefinitely with nothing in the logs.
/// Override with `TINYNET_TIMEOUT_SECS`.
const DEFAULT_TIMEOUT_SECS: u64 = 20;

fn default_timeout() -> Duration {
    std::env::var("TINYNET_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
}

/// Per-address TCP connect timeout. DNS often returns several addresses (and a
/// mix of IPv4/IPv6); without a bound, the client commits to the first one and a
/// single unreachable or tarpitting address hangs the whole request. With it, a
/// stalled address fails fast and we try the next. Override with
/// `TINYNET_CONNECT_TIMEOUT_SECS`.
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;

fn connect_timeout() -> Duration {
    std::env::var("TINYNET_CONNECT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS))
}

/// Deadline for a single connect attempt: the connect timeout, but never past
/// the overall transfer deadline when one is set.
fn attempt_deadline(connect_timeout: Duration, transfer_deadline: Option<Instant>) -> Instant {
    let by_connect = Instant::now() + connect_timeout;
    match transfer_deadline {
        Some(d) => by_connect.min(d),
        None => by_connect,
    }
}

async fn connect_first_tls(
    addrs: &[SocketAddr],
    host: &str,
    connect_timeout: Duration,
    transfer_deadline: Option<Instant>,
) -> Result<AsyncTlsStream, Error> {
    let mut last_err = None;
    for &addr in addrs {
        match AsyncTlsStream::connect(addr, host, transfer_deadline) {
            Ok(io) => match io.wait_connected(attempt_deadline(connect_timeout, transfer_deadline)).await {
                Ok(()) => return Ok(io),
                Err(err) => last_err = Some(format!("{addr}: {err}")),
            },
            Err(err) => last_err = Some(format!("{addr}: {err}")),
        }
    }
    Err(Error::RequestError(
        last_err.unwrap_or_else(|| "failed to connect".to_string()),
    ))
}

async fn connect_first_tcp(
    addrs: &[SocketAddr],
    connect_timeout: Duration,
    transfer_deadline: Option<Instant>,
) -> Result<AsyncTcpStream, Error> {
    let mut last_err = None;
    for &addr in addrs {
        match AsyncTcpStream::new(addr, transfer_deadline) {
            Ok(io) => match io.wait_connected(attempt_deadline(connect_timeout, transfer_deadline)).await {
                Ok(()) => return Ok(io),
                Err(err) => last_err = Some(format!("{addr}: {err}")),
            },
            Err(err) => last_err = Some(format!("{addr}: {err}")),
        }
    }
    Err(Error::RequestError(
        last_err.unwrap_or_else(|| "failed to connect".to_string()),
    ))
}

pub async fn send_request<T>(req: Request<T>) -> Result<(u16, Bytes), Error>
where
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    send_request_with_timeout(req, default_timeout()).await
}

pub async fn send_request_with_timeout<T>(
    req: Request<T>,
    timeout: Duration,
) -> Result<(u16, Bytes), Error>
where
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let (status, _headers, body) = send_request_with_headers_timeout(req, timeout).await?;
    Ok((status, body))
}

pub async fn send_request_with_headers<T>(
    req: Request<T>,
) -> Result<(u16, hyper::HeaderMap, Bytes), Error>
where
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    send_request_with_headers_timeout(req, default_timeout()).await
}

pub async fn send_request_with_headers_timeout<T>(
    req: Request<T>,
    timeout: Duration,
) -> Result<(u16, hyper::HeaderMap, Bytes), Error>
where
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let (host, is_https, addrs, req) = prepare_request(req)?;
    let transfer_deadline = Some(Instant::now() + timeout);
    let connect_timeout = connect_timeout();

    if is_https {
        let io = connect_first_tls(&addrs, &host, connect_timeout, transfer_deadline).await?;
        do_request(io, req).await
    } else {
        let io = connect_first_tcp(&addrs, connect_timeout, transfer_deadline).await?;
        do_request(io, req).await
    }
}

pub async fn stream_request<T>(
    req: Request<T>,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, Error>> + Send + 'static>>
where
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let (host, is_https, addrs, req) = match prepare_request(req) {
        Ok(v) => v,
        Err(err) => {
            return Box::pin(futures::stream::once(async move { Err(err) }));
        }
    };
    // Streaming responses (e.g. LLM SSE) are long-lived; an overall deadline
    // would truncate them. The connect phase is still bounded so an unreachable
    // address can't hang setup. Stalls mid-stream surface through the caller's
    // own stream handling instead.
    let transfer_deadline = None;
    let connect_timeout = connect_timeout();

    if is_https {
        match connect_first_tls(&addrs, &host, connect_timeout, transfer_deadline).await {
            Ok(io) => do_stream_request(io, req).await,
            Err(err) => Box::pin(futures::stream::once(async move { Err(err) })),
        }
    } else {
        match connect_first_tcp(&addrs, connect_timeout, transfer_deadline).await {
            Ok(io) => do_stream_request(io, req).await,
            Err(err) => Box::pin(futures::stream::once(async move { Err(err) })),
        }
    }
}

async fn do_request<Io, T>(io: Io, req: Request<T>) -> Result<(u16, hyper::HeaderMap, Bytes), Error>
where
    Io: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| Error::RequestError(e.to_string()))?;

    let request_fut = async move {
        let res = sender
            .send_request(req)
            .await
            .map_err(|e| Error::RequestError(e.to_string()))?;

        let status = res.status().as_u16();
        let headers = res.headers().clone();
        let bytes = res
            .into_body()
            .collect()
            .await
            .map_err(|e| Error::RequestError(e.to_string()))?
            .to_bytes();

        Ok((status, headers, bytes))
    };

    futures::pin_mut!(request_fut);
    futures::pin_mut!(conn);

    let mut conn_closed = false;
    loop {
        if conn_closed {
            return request_fut.await;
        }

        match futures::future::select(request_fut.as_mut(), conn.as_mut()).await {
            Either::Left((req_res, _)) => return req_res,
            Either::Right((conn_res, _)) => match conn_res {
                Ok(()) => {
                    conn_closed = true;
                }
                Err(err) => return Err(Error::RequestError(err.to_string())),
            },
        }
    }
}

async fn do_stream_request<Io, T>(
    io: Io,
    req: Request<T>,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, Error>> + Send + 'static>>
where
    Io: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let (mut sender, conn) = match hyper::client::conn::http1::handshake(io).await {
        Ok(v) => v,
        Err(e) => {
            return Box::pin(futures::stream::once(async move {
                Err(Error::RequestError(e.to_string()))
            }));
        }
    };
    let mut conn = Box::pin(conn);

    let request_fut = async {
        let res = sender
            .send_request(req)
            .await
            .map_err(|e| Error::RequestError(e.to_string()))?;
        Ok::<_, Error>(res)
    };
    futures::pin_mut!(request_fut);

    let res = match futures::future::select(request_fut, conn.as_mut()).await {
        Either::Left((Ok(res), _)) => res,
        Either::Left((Err(err), _)) => {
            return Box::pin(futures::stream::once(async move { Err(err) }));
        }
        Either::Right((Ok(()), _)) => {
            return Box::pin(futures::stream::once(async {
                Err(Error::RequestError(
                    "connection closed before response completed".to_string(),
                ))
            }));
        }
        Either::Right((Err(err), _)) => {
            return Box::pin(futures::stream::once(async move {
                Err(Error::RequestError(err.to_string()))
            }));
        }
    };

    let status = res.status().as_u16();

    if !(200..300).contains(&status) {
        // Reading the error body still requires driving the connection future,
        // exactly like the streaming path below. Awaiting `collect()` alone would
        // deadlock on a chunked error response because nothing pumps the socket.
        let collect_fut = res.into_body().collect();
        futures::pin_mut!(collect_fut);
        let collected = match futures::future::select(collect_fut.as_mut(), conn.as_mut()).await {
            Either::Left((collected, _)) => collected,
            Either::Right((Ok(()), _)) => collect_fut.await,
            Either::Right((Err(err), _)) => {
                return Box::pin(futures::stream::once(async move {
                    Err(Error::RequestError(err.to_string()))
                }));
            }
        };
        let body_bytes = match collected {
            Ok(collected) => collected.to_bytes(),
            Err(e) => {
                return Box::pin(futures::stream::once(async move {
                    Err(Error::RequestError(e.to_string()))
                }));
            }
        };
        let error_msg = String::from_utf8_lossy(&body_bytes).to_string();
        return Box::pin(futures::stream::once(async move {
            Err(Error::ServerError(status, error_msg))
        }));
    }

    let mut body = res.into_body();
    let mut conn = Some(conn);
    let s = async_stream::stream! {
        loop {
            let next_frame_fut = body.frame().fuse();
            futures::pin_mut!(next_frame_fut);

            if let Some(conn_fut) = conn.as_mut() {
                match futures::future::select(next_frame_fut, conn_fut.as_mut()).await {
                    Either::Left((frame_opt, _)) => {
                        match frame_opt {
                            Some(Ok(frame)) => {
                                if let Ok(data) = frame.into_data() {
                                    yield Ok(data);
                                }
                            }
                            Some(Err(err)) => {
                                yield Err(Error::RequestError(err.to_string()));
                                return;
                            }
                            None => return,
                        }
                    }
                    Either::Right((conn_res, _)) => match conn_res {
                        Ok(()) => {
                            conn = None;
                        }
                        Err(err) => {
                            yield Err(Error::RequestError(err.to_string()));
                            return;
                        }
                    },
                }
            } else {
                match next_frame_fut.await {
                    Some(Ok(frame)) => {
                        if let Ok(data) = frame.into_data() {
                            yield Ok(data);
                        }
                    }
                    Some(Err(err)) => {
                        yield Err(Error::RequestError(err.to_string()));
                        return;
                    }
                    None => return,
                }
            }
        }
    };

    Box::pin(s)
}

fn prepare_request<T>(
    req: Request<T>,
) -> Result<(String, bool, Vec<std::net::SocketAddr>, Request<T>), Error> {
    let uri = req.uri().clone();

    let host = uri
        .host()
        .ok_or_else(|| Error::InvalidRequest("missing host".into()))?
        .to_string();

    let is_https = uri.scheme_str() == Some("https");
    let port = uri.port_u16().unwrap_or(if is_https { 443 } else { 80 });

    let mut addrs = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|e| Error::RequestError(e.to_string()))?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err(Error::RequestError("DNS resolution failed".into()));
    }
    addrs.sort_by_key(|addr| addr.is_ipv6());

    let path_and_query = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/")
        .to_string();

    let mut req = req;
    *req.uri_mut() = path_and_query
        .parse()
        .map_err(|_| Error::InvalidRequest("invalid URI path".into()))?;

    req.headers_mut()
        .entry(HOST)
        .or_insert_with(|| HeaderValue::from_str(&host).unwrap_or(HeaderValue::from_static("")));

    Ok((host, is_https, addrs, req))
}

pub struct SseDecoder {
    buf: Vec<u8>,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn push<E, F>(&mut self, chunk: &[u8], mut on_data: F) -> Result<bool, E>
    where
        F: FnMut(&[u8]) -> Result<(), E>,
    {
        self.buf.extend_from_slice(chunk);

        let mut scan_from = 0usize;
        let mut processed = 0usize;
        while let Some(pos) = self.buf[scan_from..].windows(2).position(|w| w == b"\n\n") {
            let event_end = scan_from + pos;
            let event = &self.buf[scan_from..event_end];
            for line in event.split(|b| *b == b'\n') {
                let line = trim_ascii(line);
                if line.is_empty() {
                    continue;
                }
                if let Some(data) = line.strip_prefix(b"data: ") {
                    if data == b"[DONE]" {
                        return Ok(true);
                    }
                    on_data(data)?;
                }
            }
            processed = event_end + 2;
            scan_from = processed;
        }

        if processed > 0 {
            self.buf.drain(..processed);
        }

        Ok(false)
    }
}

impl Default for SseDecoder {
    fn default() -> Self {
        Self::new()
    }
}

pub struct NdjsonDecoder {
    buf: Vec<u8>,
}

impl NdjsonDecoder {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn push<E, F>(&mut self, chunk: &[u8], mut on_line: F) -> Result<(), E>
    where
        F: FnMut(&[u8]) -> Result<(), E>,
    {
        self.buf.extend_from_slice(chunk);

        let mut scan_from = 0usize;
        let mut processed = 0usize;
        while let Some(pos) = self.buf[scan_from..].iter().position(|b| *b == b'\n') {
            let line_end = scan_from + pos;
            let line = trim_ascii(&self.buf[scan_from..line_end]);
            if !line.is_empty() {
                on_line(line)?;
            }
            processed = line_end + 1;
            scan_from = processed;
        }

        if processed > 0 {
            self.buf.drain(..processed);
        }

        Ok(())
    }
}

impl Default for NdjsonDecoder {
    fn default() -> Self {
        Self::new()
    }
}

fn trim_ascii(mut s: &[u8]) -> &[u8] {
    while let Some(first) = s.first() {
        if !first.is_ascii_whitespace() {
            break;
        }
        s = &s[1..];
    }
    while let Some(last) = s.last() {
        if !last.is_ascii_whitespace() {
            break;
        }
        s = &s[..s.len() - 1];
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    // Bind and immediately release a port so connecting to it is refused.
    fn refused_addr() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        addr
    }

    #[tokio::test]
    async fn connect_first_falls_back_to_reachable_address() {
        let dead = refused_addr();
        // Bound but never accept()ed: the kernel still completes the handshake.
        let live = TcpListener::bind("127.0.0.1:0").unwrap();
        let live_addr = live.local_addr().unwrap();

        let addrs = vec![dead, live_addr];
        let start = Instant::now();
        let result = connect_first_tcp(
            &addrs,
            Duration::from_secs(5),
            Some(Instant::now() + Duration::from_secs(5)),
        )
        .await;

        assert!(
            result.is_ok(),
            "should fall back from the refused address to the live one: {:?}",
            result.err()
        );
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "fallback should be fast, took {:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn connect_first_errors_when_all_addresses_unreachable() {
        let result = connect_first_tcp(
            &[refused_addr()],
            Duration::from_secs(2),
            Some(Instant::now() + Duration::from_secs(2)),
        )
        .await;
        assert!(result.is_err());
    }
}
