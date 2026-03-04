use crate::Error;
use bytes::Bytes;
use http_body_util::Full;
use hyper_rustls::{ConfigBuilderExt, HttpsConnector};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};

pub fn get_http_client() -> Result<Client<HttpsConnector<HttpConnector>, Full<Bytes>>, Error> {
    let tls = rustls::ClientConfig::builder()
        .with_native_roots()
        .map_err(|_| Error::Unknown)?
        .with_no_client_auth();

    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls)
        .https_or_http()
        .enable_http1()
        .build();

    Ok(Client::builder(TokioExecutor::new()).build(https))
}
