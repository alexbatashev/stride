use std::io;
use std::net::SocketAddr;
use std::task::{Context, Poll};

use crate::buf::WriteBuf;
use crate::http::{self, Request, Response};
use crate::tcp::TcpStream;

pub trait Protocol {
    fn encode_request(&mut self, req: &Request<'_>) -> Vec<u8>;
    fn decode_request(&self, buf: &[u8]) -> io::Result<Request<'static>>;
    fn encode_response(&self, resp: &Response<'_>) -> Vec<u8>;
    fn decode_response(&self, buf: &[u8]) -> io::Result<Response<'static>>;
    fn request_length(&self, buf: &[u8]) -> Option<usize>;
    fn response_length(&self, buf: &[u8]) -> Option<usize>;
}

pub enum AnyProtocol {
    HttpV1,
    #[cfg(feature = "http2")]
    HttpV2(http::v2::Encoder),
}

impl AnyProtocol {
    pub fn is_http_v1(&self) -> bool {
        matches!(self, AnyProtocol::HttpV1)
    }
}

impl Protocol for AnyProtocol {
    fn encode_request(&mut self, req: &Request<'_>) -> Vec<u8> {
        match self {
            AnyProtocol::HttpV1 => http::v1::encode_request(req),
            #[cfg(feature = "http2")]
            AnyProtocol::HttpV2(encoder) => encoder.encode(req),
        }
    }

    fn decode_request(&self, buf: &[u8]) -> io::Result<Request<'static>> {
        match self {
            AnyProtocol::HttpV1 => http::v1::decode_request(buf)
                .map(Request::into_owned)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}"))),
            #[cfg(feature = "http2")]
            AnyProtocol::HttpV2(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "HTTP/2 request decoding is unsupported",
            )),
        }
    }

    fn encode_response(&self, resp: &Response<'_>) -> Vec<u8> {
        match self {
            AnyProtocol::HttpV1 => http::v1::encode_response(resp),
            #[cfg(feature = "http2")]
            AnyProtocol::HttpV2(_) => Vec::new(),
        }
    }

    fn decode_response(&self, buf: &[u8]) -> io::Result<Response<'static>> {
        match self {
            AnyProtocol::HttpV1 => http::v1::decode_response(buf)
                .map(Response::into_owned)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}"))),
            #[cfg(feature = "http2")]
            AnyProtocol::HttpV2(_) => http::v2::decode_response(buf),
        }
    }

    fn request_length(&self, buf: &[u8]) -> Option<usize> {
        match self {
            AnyProtocol::HttpV1 => http::v1::request_length(buf),
            #[cfg(feature = "http2")]
            AnyProtocol::HttpV2(_) => None,
        }
    }

    fn response_length(&self, buf: &[u8]) -> Option<usize> {
        match self {
            AnyProtocol::HttpV1 => http::v1::response_length(buf),
            #[cfg(feature = "http2")]
            AnyProtocol::HttpV2(_) => http::v2::response_length(buf),
        }
    }
}

pub enum AnyTransport {
    Tcp(TcpStream),
}

impl AnyTransport {
    pub async fn connect(addr: SocketAddr) -> io::Result<Self> {
        Ok(AnyTransport::Tcp(TcpStream::connect(addr).await?))
    }

    pub async fn write(&mut self, mut data: &[u8]) -> io::Result<()> {
        while !data.is_empty() {
            let n = match self {
                AnyTransport::Tcp(s) => s.write(data).await?,
            };
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "write failed"));
            }
            data = &data[n..];
        }
        Ok(())
    }

    pub async fn write_buf(&mut self, buffer: &mut WriteBuf) -> io::Result<()> {
        while !buffer.is_empty() {
            let slices = buffer.as_ioslices();
            let written = match self {
                AnyTransport::Tcp(stream) => stream.write_vectored(&slices).await?,
            };
            if written == 0 {
                return Err(io::Error::from(io::ErrorKind::WriteZero));
            }
            buffer.advance(written);
        }
        Ok(())
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            AnyTransport::Tcp(s) => s.read(buf).await,
        }
    }

    pub(crate) fn poll_read(
        &mut self,
        context: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match self {
            AnyTransport::Tcp(stream) => stream.poll_read(context, buffer),
        }
    }

    #[cfg(feature = "rustls")]
    pub(crate) fn poll_write(
        &mut self,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self {
            AnyTransport::Tcp(stream) => stream.poll_write(context, buffer),
        }
    }
}
