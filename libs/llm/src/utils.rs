use crate::Error;
use bytes::Bytes;
use http_body_util::Full;
use hyper_rustls::{ConfigBuilderExt, HttpsConnector};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
};

pub type BoxSendFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

#[derive(Clone)]
pub struct SharedExecutor(Arc<dyn hyper::rt::Executor<BoxSendFuture> + Send + Sync>);

impl std::fmt::Debug for SharedExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SharedExecutor(..)")
    }
}

impl SharedExecutor {
    pub fn new<E>(executor: E) -> Self
    where
        E: hyper::rt::Executor<BoxSendFuture> + Send + Sync + 'static,
    {
        Self(Arc::new(executor))
    }
}

impl<Fut> hyper::rt::Executor<Fut> for SharedExecutor
where
    Fut: Future + Send + 'static,
    Fut::Output: Send + 'static,
{
    fn execute(&self, fut: Fut) {
        self.0.execute(Box::pin(async move {
            let _ = fut.await;
        }));
    }
}

pub fn get_http_client<E>(
    executor: E,
) -> Result<Client<HttpsConnector<HttpConnector>, Full<Bytes>>, Error>
where
    E: hyper::rt::Executor<BoxSendFuture> + Clone + Send + Sync + 'static,
{
    let tls = rustls::ClientConfig::builder()
        .with_native_roots()
        .map_err(|_| Error::Unknown)?
        .with_no_client_auth();

    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls)
        .https_or_http()
        .enable_http1()
        .build();

    Ok(Client::builder(executor).build(https))
}
