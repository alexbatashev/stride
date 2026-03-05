mod stream;

use std::pin::Pin;
use std::net::ToSocketAddrs;

use bytes::Bytes;
use futures::FutureExt;
use futures::Stream;
use futures::future::Either;
use http_body_util::BodyExt;
use hyper::Request;
use hyper::body::Body;
use hyper::header::{HOST, HeaderValue};

use crate::Error;
use stream::{AsyncTcpStream, AsyncTlsStream};

pub(crate) async fn send_request<T>(req: Request<T>) -> Result<(u16, Bytes), Error>
where
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let (host, is_https, addrs, req) = prepare_request(req)?;

    if is_https {
        let mut last_err = None;
        for addr in addrs {
            match AsyncTlsStream::connect(addr, &host) {
                Ok(io) => return do_request(io, req).await,
                Err(err) => last_err = Some(err),
            }
        }
        Err(Error::RequestError(
            last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "failed to connect".to_string()),
        ))
    } else {
        let mut last_err = None;
        for addr in addrs {
            match AsyncTcpStream::new(addr).await {
                Ok(io) => return do_request(io, req).await,
                Err(err) => last_err = Some(err),
            }
        }
        Err(Error::RequestError(
            last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "failed to connect".to_string()),
        ))
    }
}

pub(crate) async fn stream_request<T, R, F>(
    req: Request<T>,
    f: F,
) -> Pin<Box<dyn Stream<Item = Result<R, Error>> + Send + 'static>>
where
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    R: Send + 'static,
    F: FnMut(Bytes) -> Result<R, Error> + Send + 'static,
{
    let (host, is_https, addrs, req) = match prepare_request(req) {
        Ok(v) => v,
        Err(err) => return Box::pin(futures::stream::once(async move { Err(err) })),
    };

    if is_https {
        let mut last_err = None;
        for addr in addrs {
            match AsyncTlsStream::connect(addr, &host) {
                Ok(io) => return do_stream_request(io, req, f).await,
                Err(err) => last_err = Some(err),
            }
        }
        let msg = last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "failed to connect".to_string());
        Box::pin(futures::stream::once(
            async move { Err(Error::RequestError(msg)) },
        ))
    } else {
        let mut last_err = None;
        for addr in addrs {
            match AsyncTcpStream::new(addr).await {
                Ok(io) => return do_stream_request(io, req, f).await,
                Err(err) => last_err = Some(err),
            }
        }
        let msg = last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "failed to connect".to_string());
        Box::pin(futures::stream::once(
            async move { Err(Error::RequestError(msg)) },
        ))
    }
}

async fn do_request<Io, T>(io: Io, req: Request<T>) -> Result<(u16, Bytes), Error>
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
        let bytes = res
            .into_body()
            .collect()
            .await
            .map_err(|e| Error::RequestError(e.to_string()))?
            .to_bytes();

        Ok((status, bytes))
    };

    futures::pin_mut!(request_fut);
    futures::pin_mut!(conn);

    match futures::future::select(request_fut, conn).await {
        Either::Left((req_res, _)) => req_res,
        Either::Right((conn_res, _)) => match conn_res {
            Ok(()) => Err(Error::RequestError(
                "connection closed before response completed".to_string(),
            )),
            Err(err) => Err(Error::RequestError(err.to_string())),
        },
    }
}

async fn do_stream_request<Io, T, R, F>(
    io: Io,
    req: Request<T>,
    mut f: F,
) -> Pin<Box<dyn Stream<Item = Result<R, Error>> + Send + 'static>>
where
    Io: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    R: Send + 'static,
    F: FnMut(Bytes) -> Result<R, Error> + Send + 'static,
{
    let (mut sender, conn) = match hyper::client::conn::http1::handshake(io).await {
        Ok(v) => v,
        Err(e) => {
            return Box::pin(futures::stream::once(async move {
                Err(Error::RequestError(e.to_string()))
            }))
        }
    };
    let mut conn = Box::pin(conn);

    let request_fut = sender.send_request(req);
    futures::pin_mut!(request_fut);

    let res = match futures::future::select(request_fut, conn.as_mut()).await {
        Either::Left((Ok(res), _)) => res,
        Either::Left((Err(err), _)) => {
            return Box::pin(futures::stream::once(async move {
                Err(Error::RequestError(err.to_string()))
            }))
        }
        Either::Right((Ok(()), _)) => {
            return Box::pin(futures::stream::once(async {
                Err(Error::RequestError(
                    "connection closed before response completed".to_string(),
                ))
            }))
        }
        Either::Right((Err(err), _)) => {
            return Box::pin(futures::stream::once(async move {
                Err(Error::RequestError(err.to_string()))
            }))
        }
    };

    let mut body = res.into_body();
    let s = async_stream::stream! {
        loop {
            let next_frame_fut = body.frame().fuse();
            futures::pin_mut!(next_frame_fut);

            match futures::future::select(next_frame_fut, conn.as_mut()).await {
                Either::Left((frame_opt, _)) => {
                    match frame_opt {
                        Some(Ok(frame)) => {
                            if let Ok(data) = frame.into_data() {
                                match f(data) {
                                    Ok(mapped) => yield Ok(mapped),
                                    Err(err) => {
                                        yield Err(err);
                                        return;
                                    }
                                }
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
                        yield Err(Error::RequestError(
                            "connection closed before stream completed".to_string(),
                        ));
                        return;
                    }
                    Err(err) => {
                        yield Err(Error::RequestError(err.to_string()));
                        return;
                    }
                },
            }
        }
    };

    Box::pin(s)
}

fn prepare_request<T>(req: Request<T>) -> Result<(String, bool, Vec<std::net::SocketAddr>, Request<T>), Error> {
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

    // Rewrite to origin form and ensure Host header is present.
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

pub(crate) struct SseDecoder {
    buf: Vec<u8>,
}

impl SseDecoder {
    pub(crate) fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub(crate) fn push<F>(&mut self, chunk: &[u8], mut on_data: F) -> Result<bool, Error>
    where
        F: FnMut(&[u8]) -> Result<(), Error>,
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

pub(crate) struct NdjsonDecoder {
    buf: Vec<u8>,
}

impl NdjsonDecoder {
    pub(crate) fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub(crate) fn push<F>(&mut self, chunk: &[u8], mut on_line: F) -> Result<(), Error>
    where
        F: FnMut(&[u8]) -> Result<(), Error>,
    {
        self.buf.extend_from_slice(chunk);

        let mut scan_from = 0usize;
        let mut processed = 0usize;
        while let Some(pos) = self.buf[scan_from..].iter().position(|b| *b == b'\n') {
            let line_end = scan_from + pos;
            let line = trim_ascii(&self.buf[scan_from..line_end]);
            if !line.is_empty() { on_line(line)?; }
            processed = line_end + 1;
            scan_from = processed;
        }

        if processed > 0 {
            self.buf.drain(..processed);
        }

        Ok(())
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
