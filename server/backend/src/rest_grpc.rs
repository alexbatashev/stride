use std::convert::Infallible;
use std::task::{Context, Poll};

use axum::Router;
use axum::body::{Body, Bytes, HttpBody};
use axum::http::Request;
use axum::http::header::CONTENT_TYPE;
use axum::response::Response;
use futures::ready;
use tower::{Service, make::Shared};

#[derive(Clone, Debug)]
pub struct RestGrpcService {
    rest_router: Router,
    grpc_router: Router,
    rest_ready: bool,
    grpc_ready: bool,
}

impl RestGrpcService {
    pub fn new(rest_router: Router, grpc_router: Router) -> Self {
        Self {
            rest_router,
            grpc_router,
            rest_ready: false,
            grpc_ready: false,
        }
    }

    pub fn into_make_service(self) -> Shared<Self> {
        Shared::new(self)
    }
}

impl<ReqBody> Service<Request<ReqBody>> for RestGrpcService
where
    ReqBody: HttpBody<Data = Bytes> + Send + 'static,
    ReqBody::Error: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
{
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = <Router as Service<Request<ReqBody>>>::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        loop {
            match (self.rest_ready, self.grpc_ready) {
                (true, true) => return Poll::Ready(Ok(())),
                (false, _) => {
                    ready!(<Router as Service<Request<ReqBody>>>::poll_ready(
                        &mut self.rest_router,
                        cx
                    ))?;
                    self.rest_ready = true;
                }
                (_, false) => {
                    ready!(<Router as Service<Request<ReqBody>>>::poll_ready(
                        &mut self.grpc_router,
                        cx
                    ))?;
                    self.grpc_ready = true;
                }
            }
        }
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        assert!(
            self.grpc_ready,
            "grpc service not ready; call poll_ready first"
        );
        assert!(
            self.rest_ready,
            "rest service not ready; call poll_ready first"
        );

        if is_grpc_request(&req) {
            self.grpc_ready = false;
            self.grpc_router.call(req)
        } else {
            self.rest_ready = false;
            self.rest_router.call(req)
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
