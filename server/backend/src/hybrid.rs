use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes, HttpBody};
use axum::http::Request;
use axum::http::header::CONTENT_TYPE;
use axum::response::Response;
use tower::Service;

type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub fn hybrid<MakeWeb, Grpc>(make_web: MakeWeb, grpc: Grpc) -> HybridMakeService<MakeWeb, Grpc> {
    HybridMakeService { make_web, grpc }
}

#[derive(Clone)]
pub struct HybridMakeService<MakeWeb, Grpc> {
    make_web: MakeWeb,
    grpc: Grpc,
}

impl<ConnInfo, MakeWeb, Web, Grpc> Service<ConnInfo> for HybridMakeService<MakeWeb, Grpc>
where
    MakeWeb: Service<ConnInfo, Response = Web>,
    MakeWeb::Future: Send + 'static,
    Grpc: Clone + Send + 'static,
    Web: Send + 'static,
{
    type Response = HybridService<Web, Grpc>;
    type Error = MakeWeb::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.make_web.poll_ready(cx)
    }

    fn call(&mut self, conn_info: ConnInfo) -> Self::Future {
        let web_fut = self.make_web.call(conn_info);
        let grpc = self.grpc.clone();
        Box::pin(async move {
            let web = web_fut.await?;
            Ok(HybridService { web, grpc })
        })
    }
}

pub struct HybridService<Web, Grpc> {
    web: Web,
    grpc: Grpc,
}

impl<Web, Grpc, WebBody, GrpcBody> Service<Request<Body>> for HybridService<Web, Grpc>
where
    Web: Service<Request<Body>, Response = Response<WebBody>>,
    Web::Error: Into<BoxError>,
    Web::Future: Send + 'static,
    Grpc: Service<Request<Body>, Response = Response<GrpcBody>>,
    Grpc::Error: Into<BoxError>,
    Grpc::Future: Send + 'static,
    WebBody: HttpBody<Data = Bytes> + Send + 'static,
    WebBody::Error: Into<BoxError>,
    GrpcBody: HttpBody<Data = Bytes> + Send + 'static,
    GrpcBody::Error: Into<BoxError>,
{
    type Response = Response<Body>;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.web.poll_ready(cx) {
            Poll::Ready(Ok(())) => self.grpc.poll_ready(cx).map_err(Into::into),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err.into())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        if is_grpc_request(&req) {
            let fut = self.grpc.call(req);
            Box::pin(async move {
                let response = fut.await.map_err(Into::into)?;
                Ok(response.map(Body::new))
            })
        } else {
            let fut = self.web.call(req);
            Box::pin(async move {
                let response = fut.await.map_err(Into::into)?;
                Ok(response.map(Body::new))
            })
        }
    }
}

fn is_grpc_request<B>(req: &Request<B>) -> bool {
    if req.uri().path().starts_with("/friday.core.rpc.") {
        return true;
    }

    req.headers()
        .get(CONTENT_TYPE)
        .map(|value| value.as_bytes())
        .filter(|value| value.starts_with(b"application/grpc"))
        .is_some()
}
