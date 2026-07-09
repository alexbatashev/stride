use crate::buf::IoBuf;
use crate::utils::{StrLike, find_nth};

#[cfg(feature = "client")]
pub mod body;

#[cfg(feature = "http1")]
pub mod v1;
#[cfg(feature = "http1")]
pub mod v1_conn;
#[cfg(feature = "http2")]
pub mod v2;

#[derive(Clone)]
pub enum Method<'a> {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
    HEAD,
    OPTIONS,
    CONNECT,
    TRACE,
    Custom(StrLike<'a>),
}

#[derive(Clone)]
pub enum Bytes<'a> {
    Static(&'static [u8]),
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
    Shared(IoBuf),
}

impl<'a> Bytes<'a> {
    pub fn into_owned(self) -> Bytes<'static> {
        match self {
            Bytes::Static(b) => Bytes::Static(b),
            Bytes::Borrowed(b) => Bytes::Owned(b.to_vec()),
            Bytes::Owned(b) => Bytes::Owned(b),
            Bytes::Shared(b) => Bytes::Shared(b),
        }
    }
}

impl<'a> From<&'a [u8]> for Bytes<'a> {
    fn from(value: &'a [u8]) -> Self {
        Bytes::Borrowed(value)
    }
}

impl<'a> From<&'a str> for Bytes<'a> {
    fn from(value: &'a str) -> Self {
        Bytes::Borrowed(value.as_bytes())
    }
}

impl<'a> From<Vec<u8>> for Bytes<'a> {
    fn from(value: Vec<u8>) -> Self {
        Bytes::Owned(value)
    }
}

impl From<IoBuf> for Bytes<'static> {
    fn from(value: IoBuf) -> Self {
        Bytes::Shared(value)
    }
}

impl<'a> From<String> for Bytes<'a> {
    fn from(value: String) -> Self {
        Bytes::Owned(value.into_bytes())
    }
}

impl AsRef<[u8]> for Bytes<'_> {
    fn as_ref(&self) -> &[u8] {
        match self {
            Bytes::Static(b) => b,
            Bytes::Borrowed(b) => b,
            Bytes::Owned(b) => b,
            Bytes::Shared(b) => b.as_ref(),
        }
    }
}

#[derive(Clone)]
pub struct Request<'a> {
    pub method: Method<'a>,
    pub scheme: StrLike<'a>,
    pub host: StrLike<'a>,
    pub target: StrLike<'a>,
    pub headers: Vec<(StrLike<'a>, StrLike<'a>)>,
    pub body: Option<Bytes<'a>>,
}

impl<'a> Request<'a> {
    pub fn into_owned(self) -> Request<'static> {
        Request {
            method: self.method.into_owned(),
            scheme: self.scheme.into_owned(),
            host: self.host.into_owned(),
            target: self.target.into_owned(),
            headers: self
                .headers
                .into_iter()
                .map(|(name, value)| (name.into_owned(), value.into_owned()))
                .collect(),
            body: self.body.map(Bytes::into_owned),
        }
    }

    pub fn header(mut self, name: impl Into<StrLike<'a>>, value: impl Into<StrLike<'a>>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn body(mut self, data: impl Into<Bytes<'a>>) -> Self {
        self.body = Some(data.into());
        self
    }
}

pub struct Response<'a> {
    pub status: u16,
    pub reason: StrLike<'a>,
    pub headers: Vec<(StrLike<'a>, StrLike<'a>)>,
    pub body: Option<Bytes<'a>>,
}

impl<'a> Method<'a> {
    pub fn into_owned(self) -> Method<'static> {
        match self {
            Method::GET => Method::GET,
            Method::POST => Method::POST,
            Method::PUT => Method::PUT,
            Method::DELETE => Method::DELETE,
            Method::PATCH => Method::PATCH,
            Method::HEAD => Method::HEAD,
            Method::OPTIONS => Method::OPTIONS,
            Method::CONNECT => Method::CONNECT,
            Method::TRACE => Method::TRACE,
            Method::Custom(s) => Method::Custom(s.into_owned()),
        }
    }
}

impl<'a> Response<'a> {
    pub fn into_owned(self) -> Response<'static> {
        Response {
            status: self.status,
            reason: self.reason.into_owned(),
            headers: self
                .headers
                .into_iter()
                .map(|(name, value)| (name.into_owned(), value.into_owned()))
                .collect(),
            body: self.body.map(Bytes::into_owned),
        }
    }
}

impl TryFrom<&'static str> for Request<'static> {
    type Error = std::io::Error;

    fn try_from(value: &'static str) -> Result<Self, Self::Error> {
        let (scheme, after_scheme): (&'static str, &'static str) =
            if let Some(stripped) = value.strip_prefix("https://") {
                ("https", stripped)
            } else if let Some(stripped) = value.strip_prefix("http://") {
                ("http", stripped)
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Invalid URL format",
                ));
            };

        let (host, target): (&'static str, &'static str) =
            if let Some(pos) = find_nth(after_scheme, '/', 1) {
                (&after_scheme[..pos], &after_scheme[pos..])
            } else {
                (after_scheme, "/")
            };

        Ok(Request {
            method: Method::GET,
            scheme: scheme.into(),
            host: host.into(),
            target: target.into(),
            headers: Vec::new(),
            body: None,
        })
    }
}
