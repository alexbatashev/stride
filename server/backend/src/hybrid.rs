use axum::http::{Request, header::CONTENT_TYPE};
use tower::{MakeService, Service, steer::Steer};

pub fn hybrid<Web, Grpc, B>(web: Web, grpc: Grpc) -> impl Service<Request<B>>
where
    Web: MakeService,
    Grpc: MakeService,
{
    Steer::new(
        vec![web.into_service(), grpc.into_service()],
        |req: &Request<B>, _services: &[_]| {
            return 0;
        },
    )
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
