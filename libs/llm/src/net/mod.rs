mod stream;

use std::net::ToSocketAddrs;

use bytes::Bytes;
use http_body_util::BodyExt;
use futures::future::Either;
use hyper::Request;
use hyper::body::Body;
use hyper::header::{HeaderValue, HOST};

use stream::{AsyncTcpStream, AsyncTlsStream};
use crate::Error;

pub(crate) async fn send_request<T>(req: Request<T>) -> Result<(u16, Bytes), Error>
where
    T: Body + Send + 'static,
    T::Data: Send,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
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

    // Rewrite to origin form and ensure Host header is present
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
