use crate::Error;
use futures::{Stream, future::BoxFuture};
use std::pin::Pin;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

pub trait HttpTransport: Send + Sync {
    fn request<'a>(&'a self, request: HttpRequest) -> BoxFuture<'a, Result<HttpResponse, Error>>;

    fn request_stream<'a>(
        &'a self,
        request: HttpRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<Vec<u8>, Error>> + Send + 'a>>;
}

#[derive(Clone)]
pub struct TransportHandle(pub Arc<dyn HttpTransport>);

impl std::fmt::Debug for TransportHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TransportHandle(..)")
    }
}

impl TransportHandle {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self(transport)
    }
}
