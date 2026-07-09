use crate::buf::WriteBuf;
use crate::http::body::{Body, StreamingResponse};
use crate::http::{Request, Response};
use crate::protocol::{AnyProtocol, AnyTransport, Protocol};
#[cfg(feature = "rustls")]
use crate::tls::Tls;
use crate::utils::find_nth;
use std::io;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

mod pool;
use pool::{Pool, PoolKey};

pub struct Client {
    io: Option<ClientIo>,
    protocol: AnyProtocol,
    config: ClientConfig,
    pool: Arc<Pool>,
    key: PoolKey,
    authority: String,
    reusable: bool,
    pooled: bool,
}

impl Clone for Client {
    fn clone(&self) -> Self {
        Self {
            io: None,
            protocol: AnyProtocol::HttpV1,
            config: self.config,
            pool: Arc::clone(&self.pool),
            key: self.key.clone(),
            authority: self.authority.clone(),
            reusable: false,
            pooled: false,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ClientConfig {
    pub follow_redirects: bool,
    pub protocol_upgrades: bool,
    pub connect_timeout: Option<Duration>,
    pub request_timeout: Option<Duration>,
    pub pool_idle_timeout: Duration,
    pub pool_max_idle_per_host: usize,
    pub alpn: &'static [&'static [u8]],
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            follow_redirects: true,
            protocol_upgrades: cfg!(feature = "http2"),
            connect_timeout: timeout_from_env("TINYNET_CONNECT_TIMEOUT_SECS"),
            request_timeout: timeout_from_env("TINYNET_TIMEOUT_SECS"),
            pool_idle_timeout: Duration::from_secs(90),
            pool_max_idle_per_host: 8,
            alpn: &[b"http/1.1"],
        }
    }
}

const MAX_REDIRECTS: usize = 10;

pub(crate) enum ClientIo {
    Plain(AnyTransport),
    #[cfg(feature = "rustls")]
    Tls {
        transport: AnyTransport,
        tls: Box<Tls>,
        out: Vec<u8>,
        out_pos: usize,
        incoming: Box<[u8]>,
    },
}

impl ClientIo {
    async fn write(&mut self, data: &[u8]) -> io::Result<()> {
        match self {
            ClientIo::Plain(t) => t.write(data).await,
            #[cfg(feature = "rustls")]
            ClientIo::Tls {
                transport,
                tls,
                out,
                ..
            } => {
                let mut remaining = data;
                while !remaining.is_empty() {
                    let written = tls.write(remaining)?;
                    if written == 0 {
                        return Err(io::Error::from(io::ErrorKind::WriteZero));
                    }
                    flush_tls(transport, tls, out).await?;
                    remaining = &remaining[written..];
                }
                Ok(())
            }
        }
    }

    async fn write_buf(&mut self, buffer: &mut WriteBuf) -> io::Result<()> {
        match self {
            ClientIo::Plain(transport) => transport.write_buf(buffer).await,
            #[cfg(feature = "rustls")]
            ClientIo::Tls { .. } => self.write(&buffer.concat()).await,
        }
    }

    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            ClientIo::Plain(t) => t.read(buf).await,
            #[cfg(feature = "rustls")]
            ClientIo::Tls {
                transport,
                tls,
                out,
                ..
            } => loop {
                match tls.read(buf) {
                    Ok(n) => return Ok(n),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        let mut tmp = [0u8; 4096];
                        let n = transport.read(&mut tmp).await?;
                        if n == 0 {
                            return tls.read_eof(buf);
                        }
                        tls.read_tls(&tmp[..n])?;
                        flush_tls(transport, tls, out).await?;
                    }
                    Err(e) => return Err(e),
                }
            },
        }
    }

    pub(crate) fn poll_read(
        &mut self,
        context: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match self {
            ClientIo::Plain(transport) => transport.poll_read(context, buffer),
            #[cfg(feature = "rustls")]
            ClientIo::Tls {
                transport,
                tls,
                out,
                out_pos,
                incoming,
            } => loop {
                while *out_pos < out.len() {
                    match transport.poll_write(context, &out[*out_pos..]) {
                        Poll::Ready(Ok(0)) => {
                            return Poll::Ready(Err(io::Error::from(io::ErrorKind::WriteZero)));
                        }
                        Poll::Ready(Ok(written)) => *out_pos += written,
                        Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                        Poll::Pending => return Poll::Pending,
                    }
                }
                out.clear();
                *out_pos = 0;

                match tls.read(buffer) {
                    Ok(read) => return Poll::Ready(Ok(read)),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => return Poll::Ready(Err(error)),
                }
                match transport.poll_read(context, incoming) {
                    Poll::Ready(Ok(0)) => return Poll::Ready(tls.read_eof(buffer)),
                    Poll::Ready(Ok(read)) => {
                        if let Err(error) = tls.read_tls(&incoming[..read]) {
                            return Poll::Ready(Err(error));
                        }
                        if let Err(error) = tls.write_tls(out) {
                            return Poll::Ready(Err(error));
                        }
                    }
                    Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                    Poll::Pending => return Poll::Pending,
                }
            },
        }
    }
}

#[cfg(feature = "rustls")]
async fn flush_tls(
    transport: &mut AnyTransport,
    tls: &mut Tls,
    out: &mut Vec<u8>,
) -> io::Result<()> {
    out.clear();
    tls.write_tls(out)?;
    transport.write(out).await?;
    out.clear();
    Ok(())
}

impl Client {
    /// Creates an established connection to a specified hostname. Must include
    /// protocol and domain name like this: `https://example.com`.
    pub async fn new(base: &str) -> Result<Self, io::Error> {
        Self::with_config(base, ClientConfig::default()).await
    }

    pub async fn with_config(base: &str, config: ClientConfig) -> Result<Self, io::Error> {
        let scheme_pos = find_nth(base, ':', 1).ok_or(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid address",
        ))?;

        let base_pos = find_nth(base, '/', 2).ok_or(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid address",
        ))? + 1;

        let scheme = &base[..scheme_pos];
        let mut host = &base[base_pos..];
        if host.ends_with('/') {
            host = &host[..host.len() - 1];
        }

        Self::from_parts_with_config(scheme, host, config).await
    }

    pub async fn from_parts(scheme: &str, host: &str) -> Result<Self, io::Error> {
        Self::from_parts_with_config(scheme, host, ClientConfig::default()).await
    }

    pub async fn from_parts_with_config(
        scheme: &str,
        host: &str,
        config: ClientConfig,
    ) -> Result<Self, io::Error> {
        Self::from_parts_with_pool(scheme, host, config, Arc::new(Pool::default())).await
    }

    async fn from_parts_with_pool(
        scheme: &str,
        host: &str,
        config: ClientConfig,
        pool: Arc<Pool>,
    ) -> Result<Self, io::Error> {
        let (dns_host, port) = host_and_port(scheme, host)?;
        let key = PoolKey {
            scheme: scheme.to_owned(),
            host: dns_host.to_owned(),
            port,
        };
        let pooled_transport = (scheme == "http")
            .then(|| pool.checkout(&key, config.pool_idle_timeout))
            .flatten();
        let pooled = pooled_transport.is_some();
        let transport = if let Some(transport) = pooled_transport {
            transport
        } else {
            let addresses = crate::dns::resolve(dns_host, port).await?;
            let mut last_error = None;
            let mut transport = None;
            for address in addresses {
                let connected = match config.connect_timeout {
                    Some(duration) => {
                        match crate::utils::timeout(duration, AnyTransport::connect(address)).await
                        {
                            Ok(result) => result,
                            Err(_) => Err(timed_out("connection timed out")),
                        }
                    }
                    None => AnyTransport::connect(address).await,
                };
                match connected {
                    Ok(connected) => {
                        transport = Some(connected);
                        break;
                    }
                    Err(error) => last_error = Some(error),
                }
            }
            transport.ok_or_else(|| {
                last_error.unwrap_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::AddrNotAvailable,
                        "host resolved no addresses",
                    )
                })
            })?
        };
        let io = match scheme {
            "http" => ClientIo::Plain(transport),
            "https" => {
                #[cfg(feature = "rustls")]
                {
                    let mut tls = Tls::client(tls_host(host), config.alpn)?;
                    let mut transport = transport;
                    let mut out = Vec::with_capacity(4096);
                    flush_tls(&mut transport, &mut tls, &mut out).await?;
                    while tls.is_handshaking() {
                        let mut buf = [0u8; 4096];
                        let n = transport.read(&mut buf).await?;
                        if n == 0 {
                            return Err(io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                "TLS connection closed",
                            ));
                        }
                        tls.read_tls(&buf[..n])?;
                        flush_tls(&mut transport, &mut tls, &mut out).await?;
                    }
                    ClientIo::Tls {
                        transport,
                        tls: Box::new(tls),
                        out,
                        out_pos: 0,
                        incoming: vec![0; 16 * 1024].into_boxed_slice(),
                    }
                }
                #[cfg(not(feature = "rustls"))]
                {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "HTTPS requires rustls feature",
                    ));
                }
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Unsupported URL scheme",
                ));
            }
        };

        let protocol = AnyProtocol::HttpV1;

        Ok(Client {
            io: Some(io),
            protocol,
            config,
            pool,
            key,
            authority: host.to_owned(),
            reusable: true,
            pooled,
        })
    }

    pub async fn send(&mut self, req: &Request<'_>) -> Result<Response<'static>, io::Error> {
        if let Some(duration) = self.config.request_timeout {
            return crate::utils::timeout(duration, self.send_unbounded(req))
                .await
                .map_err(|_| timed_out("request timed out"))?;
        }
        self.send_unbounded(req).await
    }

    async fn send_unbounded(&mut self, req: &Request<'_>) -> Result<Response<'static>, io::Error> {
        self.ensure_connection().await?;
        let mut req = req.clone().into_owned();

        for _ in 0..MAX_REDIRECTS {
            let resp = self.send_once(&req).await?;
            if !self.config.follow_redirects || !is_redirect(resp.status) {
                return Ok(resp);
            }

            let Some(location) = redirect_location(&resp) else {
                return Ok(resp);
            };
            req = redirected_request(&req, location)?;
            *self = Client::from_parts_with_pool(
                req.scheme.as_ref(),
                req.host.as_ref(),
                self.config,
                Arc::clone(&self.pool),
            )
            .await?;
        }

        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "redirect limit exceeded",
        ))
    }

    pub async fn send_streaming<'a>(
        &'a mut self,
        req: &Request<'_>,
    ) -> io::Result<StreamingResponse<'a>> {
        self.ensure_connection().await?;
        if !self.protocol.is_http_v1() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "streaming currently requires HTTP/1",
            ));
        }
        let io = self.io.as_mut().unwrap();
        self.reusable = false;
        let mut request = WriteBuf::new();
        crate::http::v1::encode_request_into(req, &mut request);
        io.write_buf(&mut request).await?;
        let mut body = Body::new(io, self.config.request_timeout, &mut self.reusable);
        let head = body.head().await?;
        Ok(StreamingResponse { head, body })
    }

    async fn ensure_connection(&mut self) -> io::Result<()> {
        if self.io.is_some() {
            return Ok(());
        }
        let replacement = Self::from_parts_with_pool(
            &self.key.scheme,
            &self.authority,
            self.config,
            Arc::clone(&self.pool),
        )
        .await?;
        *self = replacement;
        Ok(())
    }

    async fn send_once(&mut self, req: &Request<'_>) -> Result<Response<'static>, io::Error> {
        let retry = self.pooled;
        let result = self.send_once_inner(req).await;
        if result.is_err() && retry {
            self.reusable = false;
            self.io.take();
            let replacement = Self::from_parts_with_pool(
                &self.key.scheme,
                &self.authority,
                self.config,
                Arc::clone(&self.pool),
            )
            .await?;
            *self = replacement;
            return self.send_once_inner(req).await;
        }
        self.pooled = false;
        result
    }

    async fn send_once_inner(&mut self, req: &Request<'_>) -> Result<Response<'static>, io::Error> {
        if self.config.protocol_upgrades
            && self.protocol.is_http_v1()
            && req.scheme.as_ref() == "http"
        {
            self.reusable = false;
            self.io
                .as_mut()
                .unwrap()
                .write(&encode_upgrade_request(req))
                .await?;
            let (resp, extra) = self.read_response_with_extra().await?;
            if resp.status == 101 && is_http2_upgrade(&resp) {
                self.finish_http2_upgrade().await?;
                return self.read_response(extra).await;
            }
            return Ok(resp);
        }

        if self.protocol.is_http_v1() {
            let response = self.send_streaming(req).await?;
            let status = response.head.status;
            let reason = shared_text(response.head.reason)?;
            let headers = response
                .head
                .headers
                .into_iter()
                .map(|(name, value)| Ok((shared_text(name)?.into(), shared_text(value)?.into())))
                .collect::<io::Result<_>>()?;
            let body = response.body.collect().await?;
            return Ok(Response {
                status,
                reason: reason.into(),
                headers,
                body: (!matches!(status, 100..=199 | 204 | 304)).then_some(body),
            });
        }

        let data = self.protocol.encode_request(req);
        self.io.as_mut().unwrap().write(&data).await?;
        self.read_response(Vec::new()).await
    }

    async fn read_response(&mut self, initial: Vec<u8>) -> Result<Response<'static>, io::Error> {
        Ok(self.read_response_with_extra_from(initial).await?.0)
    }

    async fn read_response_with_extra(
        &mut self,
    ) -> Result<(Response<'static>, Vec<u8>), io::Error> {
        self.read_response_with_extra_from(Vec::new()).await
    }

    async fn read_response_with_extra_from(
        &mut self,
        initial: Vec<u8>,
    ) -> Result<(Response<'static>, Vec<u8>), io::Error> {
        let mut buf = initial;
        let mut tmp = [0u8; 4096];
        loop {
            if let Some(len) = self.protocol.response_length(&buf) {
                let extra = buf.split_off(len);
                return Ok((self.protocol.decode_response(&buf)?, extra));
            }

            let n = self.io.as_mut().unwrap().read(&mut tmp).await?;
            if n == 0 {
                return Ok((self.protocol.decode_response(&buf)?, Vec::new()));
            }
            buf.extend_from_slice(&tmp[..n]);
        }
    }

    #[cfg(feature = "http2")]
    async fn finish_http2_upgrade(&mut self) -> Result<(), io::Error> {
        let mut data = Vec::new();
        data.extend_from_slice(crate::http::v2::CONNECTION_PREFACE);
        data.extend_from_slice(&crate::http::v2::SETTINGS_FRAME);
        self.io.as_mut().unwrap().write(&data).await?;
        self.protocol = AnyProtocol::HttpV2(crate::http::v2::Encoder::after_upgrade());
        Ok(())
    }

    #[cfg(not(feature = "http2"))]
    async fn finish_http2_upgrade(&mut self) -> Result<(), io::Error> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "HTTP/2 requires http2 feature",
        ))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if !self.reusable {
            return;
        }
        if let Some(ClientIo::Plain(transport)) = self.io.take() {
            self.pool.checkin(
                self.key.clone(),
                transport,
                self.config.pool_max_idle_per_host,
            );
        }
    }
}

fn timeout_from_env(name: &str) -> Option<Duration> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
}

fn timed_out(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, message)
}

fn shared_text(buffer: crate::buf::IoBuf) -> io::Result<String> {
    std::str::from_utf8(buffer.as_ref())
        .map(str::to_owned)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 HTTP text"))
}

fn encode_upgrade_request(req: &Request<'_>) -> Vec<u8> {
    let mut req = req.clone();
    if !has_header(&req.headers, "upgrade") {
        req.headers.push(("Upgrade".into(), "h2c".into()));
    }
    if !has_header(&req.headers, "connection") {
        req.headers
            .push(("Connection".into(), "Upgrade, HTTP2-Settings".into()));
    }
    if !has_header(&req.headers, "http2-settings") {
        req.headers.push(("HTTP2-Settings".into(), "".into()));
    }
    crate::http::v1::encode_request(&req)
}

fn is_http2_upgrade(resp: &Response<'_>) -> bool {
    resp.headers.iter().any(|(name, value)| {
        name.as_ref().eq_ignore_ascii_case("upgrade") && value.as_ref().eq_ignore_ascii_case("h2c")
    })
}

fn has_header(headers: &[(crate::utils::StrLike, crate::utils::StrLike)], needle: &str) -> bool {
    headers
        .iter()
        .any(|(name, _)| name.as_ref().eq_ignore_ascii_case(needle))
}

pub async fn send<R>(req: R) -> Result<Response<'static>, io::Error>
where
    R: TryInto<Request<'static>>,
    R::Error: std::error::Error + Send + Sync + 'static,
{
    let req: Request = req
        .try_into()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;

    let mut client = Client::from_parts(req.scheme.as_ref(), req.host.as_ref()).await?;

    client.send(&req).await
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn redirect_location<'a>(resp: &'a Response<'_>) -> Option<&'a str> {
    resp.headers
        .iter()
        .find(|(name, _)| name.as_ref().eq_ignore_ascii_case("location"))
        .map(|(_, value)| value.as_ref())
}

fn redirected_request(req: &Request<'_>, location: &str) -> Result<Request<'static>, io::Error> {
    let (scheme, host, target) = redirect_parts(req, location)?;
    Ok(Request {
        method: req.method.clone().into_owned(),
        scheme: scheme.into(),
        host: host.into(),
        target: target.into(),
        headers: req
            .headers
            .iter()
            .cloned()
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect(),
        body: req.body.clone().map(crate::http::Bytes::into_owned),
    })
}

fn redirect_parts(req: &Request, location: &str) -> Result<(String, String, String), io::Error> {
    if location.starts_with("http://") || location.starts_with("https://") {
        return parse_url(location);
    }

    let target = if location.starts_with('/') {
        location.to_owned()
    } else {
        let current = req.target.as_ref();
        let base = current.rfind('/').map_or("/", |pos| &current[..pos + 1]);
        format!("{}{}", base, location)
    };

    Ok((
        req.scheme.as_ref().to_owned(),
        req.host.as_ref().to_owned(),
        target,
    ))
}

fn parse_url(url: &str) -> Result<(String, String, String), io::Error> {
    let scheme_pos = find_nth(url, ':', 1).ok_or(io::Error::new(
        io::ErrorKind::InvalidInput,
        "Invalid redirect URL",
    ))?;
    let after_scheme = url[scheme_pos + 1..]
        .strip_prefix("//")
        .ok_or(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid redirect URL",
        ))?;
    let (host, target) = if let Some(pos) = find_nth(after_scheme, '/', 1) {
        (&after_scheme[..pos], &after_scheme[pos..])
    } else {
        (after_scheme, "/")
    };

    Ok((
        url[..scheme_pos].to_owned(),
        host.to_owned(),
        target.to_owned(),
    ))
}

fn host_and_port<'a>(scheme: &str, authority: &'a str) -> io::Result<(&'a str, u16)> {
    let default_port = if scheme == "https" { 443 } else { 80 };
    if let Some(bracketed) = authority.strip_prefix('[') {
        let end = bracketed
            .find(']')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid IPv6 authority"))?;
        let host = &bracketed[..end];
        let remainder = &bracketed[end + 1..];
        let port = match remainder.strip_prefix(':') {
            Some(port) => parse_port(port)?,
            None if remainder.is_empty() => default_port,
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid authority",
                ));
            }
        };
        return Ok((host, port));
    }
    if let Some((host, port)) = authority.rsplit_once(':')
        && !host.contains(':')
    {
        return Ok((host, parse_port(port)?));
    }
    Ok((authority, default_port))
}

fn parse_port(port: &str) -> io::Result<u16> {
    port.parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid port"))
}

#[cfg(feature = "rustls")]
fn tls_host(host: &str) -> &str {
    host_and_port("https", host).map_or(host, |(host, _)| host)
}

#[cfg(test)]
mod e2e {
    use super::*;
    use crate::http::{Method, Request};

    async fn start_server() -> u16 {
        use axum::http::{StatusCode, header};
        use axum::routing::{get, post};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let app = axum::Router::new()
            .route("/", get(|| async { "hello" }))
            .route("/echo", post(|body: axum::body::Bytes| async move { body }))
            .route(
                "/redirect",
                get(|| async { (StatusCode::FOUND, [(header::LOCATION, "/")]) }),
            )
            .route("/missing", get(|| async { StatusCode::NOT_FOUND }));

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        port
    }

    async fn open(
        port: u16,
        method: Method<'static>,
        path: &'static str,
    ) -> (Client, Request<'static>) {
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
        let req = Request {
            method,
            scheme: "http".into(),
            host: format!("127.0.0.1:{}", port).into(),
            target: path.into(),
            headers: vec![],
            body: None,
        };
        (
            Client::from_parts("http", &addr.to_string()).await.unwrap(),
            req,
        )
    }

    #[tokio::test]
    async fn get_ok() {
        let port = start_server().await;
        let (mut client, req) = open(port, Method::GET, "/").await;
        let resp = client.send(&req).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body.unwrap().as_ref(), b"hello");
    }

    #[tokio::test]
    async fn get_not_found() {
        let port = start_server().await;
        let (mut client, req) = open(port, Method::GET, "/missing").await;
        let resp = client.send(&req).await.unwrap();
        assert_eq!(resp.status, 404);
    }

    #[tokio::test]
    async fn post_echo() {
        let port = start_server().await;
        let (mut client, req) = open(port, Method::POST, "/echo").await;
        let req = req.body("ping");
        let resp = client.send(&req).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body.unwrap().as_ref(), b"ping");
    }

    #[tokio::test]
    async fn client_follows_redirects_by_default() {
        let port = start_server().await;
        let (mut client, req) = open(port, Method::GET, "/redirect").await;
        let resp = client.send(&req).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body.unwrap().as_ref(), b"hello");
    }

    #[tokio::test]
    async fn client_config_can_disable_redirects() {
        let port = start_server().await;
        let (mut client, req) = open(port, Method::GET, "/redirect").await;
        client.config.follow_redirects = false;
        let resp = client.send(&req).await.unwrap();
        assert_eq!(resp.status, 302);
    }

    #[tokio::test]
    async fn freestanding_send_follows_redirects() {
        let port = start_server().await;
        let request = Request {
            method: Method::GET,
            scheme: "http".into(),
            host: format!("127.0.0.1:{port}").into(),
            target: "/redirect".into(),
            headers: vec![],
            body: None,
        };
        let resp = send(request).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body.unwrap().as_ref(), b"hello");
    }

    #[cfg(feature = "rustls")]
    #[tokio::test]
    async fn google_https() {
        let resp = send("https://google.com").await.unwrap();
        assert!(resp.status >= 200 && resp.status < 400);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Request;
    use crate::protocol::AnyProtocol;
    use crate::tcp::{TcpListener, TcpStream};

    #[cfg(feature = "http2")]
    async fn read_until(conn: &mut TcpStream, needle: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = [0u8; 128];
        loop {
            let n = conn.read(&mut buf).await.unwrap();
            assert!(n > 0);
            out.extend_from_slice(&buf[..n]);
            if out.windows(needle.len()).any(|w| w == needle) {
                return out;
            }
        }
    }

    #[cfg(feature = "http2")]
    #[tokio::test]
    async fn client_upgrades_to_http2_by_default() {
        let mut listener = TcpListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let req = read_until(&mut conn, b"\r\n\r\n").await;
            assert!(
                req.windows(b"Upgrade: h2c".len())
                    .any(|w| w == b"Upgrade: h2c")
            );
            assert!(
                req.windows(b"HTTP2-Settings:".len())
                    .any(|w| w == b"HTTP2-Settings:")
            );

            conn.write(&[
                b'H', b'T', b'T', b'P', b'/', b'1', b'.', b'1', b' ', b'1', b'0', b'1', b' ', b'S',
                b'w', b'i', b't', b'c', b'h', b'i', b'n', b'g', b' ', b'P', b'r', b'o', b't', b'o',
                b'c', b'o', b'l', b's', b'\r', b'\n', b'U', b'p', b'g', b'r', b'a', b'd', b'e',
                b':', b' ', b'h', b'2', b'c', b'\r', b'\n', b'C', b'o', b'n', b'n', b'e', b'c',
                b't', b'i', b'o', b'n', b':', b' ', b'U', b'p', b'g', b'r', b'a', b'd', b'e',
                b'\r', b'\n', b'\r', b'\n', 0, 0, 1, 1, 4, 0, 0, 0, 1, 0x88, 0, 0, 5, 0, 1, 0, 0,
                0, 1, b'h', b'e', b'l', b'l', b'o',
            ])
            .await
            .unwrap();
        });

        let mut client = Client::from_parts("http", &addr.to_string()).await.unwrap();
        let req = Request::try_from("http://example.com/").unwrap();
        let resp = client.send(&req).await.unwrap();

        assert_eq!(resp.status, 200);
        assert_eq!(resp.body.unwrap().as_ref(), b"hello");
        assert!(matches!(client.protocol, AnyProtocol::HttpV2(_)));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn client_streams_chunked_response() {
        let mut listener = TcpListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            let mut request = [0; 512];
            connection.read(&mut request).await.unwrap();
            connection
                .write(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n")
                .await
                .unwrap();
            crate::sleep(std::time::Duration::from_millis(20)).await;
            connection.write(b"5\r\nworld\r\n0\r\n\r\n").await.unwrap();
        });
        let mut client = Client::from_parts_with_config(
            "http",
            &addr.to_string(),
            ClientConfig {
                protocol_upgrades: false,
                ..ClientConfig::default()
            },
        )
        .await
        .unwrap();
        let request = Request {
            method: crate::http::Method::GET,
            scheme: "http".into(),
            host: addr.to_string().into(),
            target: "/".into(),
            headers: vec![],
            body: None,
        };

        let mut response = client.send_streaming(&request).await.unwrap();
        assert_eq!(response.head.status, 200);
        assert_eq!(
            response.body.chunk().await.unwrap().unwrap().as_ref(),
            b"hello"
        );
        assert_eq!(
            response.body.chunk().await.unwrap().unwrap().as_ref(),
            b"world"
        );
        assert!(response.body.chunk().await.unwrap().is_none());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn request_timeout_covers_response_body() {
        let mut listener = TcpListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            let mut request = [0; 512];
            connection.read(&mut request).await.unwrap();
            connection
                .write(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n")
                .await
                .unwrap();
            crate::sleep(Duration::from_millis(100)).await;
            connection.write(b"hello").await.unwrap();
        });
        let mut client = Client::from_parts_with_config(
            "http",
            &addr.to_string(),
            ClientConfig {
                protocol_upgrades: false,
                request_timeout: Some(Duration::from_millis(20)),
                ..ClientConfig::default()
            },
        )
        .await
        .unwrap();
        let request = Request {
            method: crate::http::Method::GET,
            scheme: "http".into(),
            host: addr.to_string().into(),
            target: "/".into(),
            headers: vec![],
            body: None,
        };

        assert_eq!(
            client.send(&request).await.err().unwrap().kind(),
            io::ErrorKind::TimedOut
        );
        drop(server);
    }

    #[test]
    fn client_config_can_disable_protocol_upgrades() {
        assert!(ClientConfig::default().protocol_upgrades == cfg!(feature = "http2"));

        let config = ClientConfig {
            protocol_upgrades: false,
            ..ClientConfig::default()
        };
        assert!(!config.protocol_upgrades);
    }

    #[tokio::test]
    async fn client_reuse_connection() {
        let mut listener = TcpListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 512];

            for body in [b"first" as &[u8], b"second"] {
                conn.read(&mut buf).await.unwrap();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    core::str::from_utf8(body).unwrap()
                );
                conn.write(resp.as_bytes()).await.unwrap();
            }
        });

        let mut client = Client::from_parts("http", &addr.to_string()).await.unwrap();
        let req = Request::try_from("http://example.com/").unwrap();

        let r1 = client.send(&req).await.unwrap();
        assert_eq!(r1.status, 200);
        assert_eq!(r1.body.unwrap().as_ref(), b"first");

        let r2 = client.send(&req).await.unwrap();
        assert_eq!(r2.status, 200);
        assert_eq!(r2.body.unwrap().as_ref(), b"second");

        server.await.unwrap();
    }

    #[tokio::test]
    async fn cloned_clients_share_idle_connections() {
        let mut listener = TcpListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            let mut request = [0; 512];
            for _ in 0..2 {
                connection.read(&mut request).await.unwrap();
                connection
                    .write(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                    .await
                    .unwrap();
            }
        });
        let config = ClientConfig {
            protocol_upgrades: false,
            ..ClientConfig::default()
        };
        let mut first = Client::from_parts_with_config("http", &addr.to_string(), config)
            .await
            .unwrap();
        let mut second = first.clone();
        let request = Request {
            method: crate::http::Method::GET,
            scheme: "http".into(),
            host: addr.to_string().into(),
            target: "/".into(),
            headers: vec![],
            body: None,
        };

        first.send(&request).await.unwrap();
        drop(first);
        second.send(&request).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn stale_pooled_connection_retries_fresh() {
        let mut listener = TcpListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut request = [0; 512];
            let (mut stale, _) = listener.accept().await.unwrap();
            stale.read(&mut request).await.unwrap();
            stale
                .write(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await
                .unwrap();
            drop(stale);

            let (mut fresh, _) = listener.accept().await.unwrap();
            fresh.read(&mut request).await.unwrap();
            fresh
                .write(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nfresh")
                .await
                .unwrap();
        });
        let config = ClientConfig {
            protocol_upgrades: false,
            ..ClientConfig::default()
        };
        let mut first = Client::from_parts_with_config("http", &addr.to_string(), config)
            .await
            .unwrap();
        let mut second = first.clone();
        let request = Request {
            method: crate::http::Method::GET,
            scheme: "http".into(),
            host: addr.to_string().into(),
            target: "/".into(),
            headers: vec![],
            body: None,
        };

        first.send(&request).await.unwrap();
        drop(first);
        let response = second.send(&request).await.unwrap();
        assert_eq!(response.body.unwrap().as_ref(), b"fresh");
        server.await.unwrap();
    }
}
