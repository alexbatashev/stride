use friday::grpc::generated::friday::core::rpc::{
    AuthReply, LoginRequest, LogoutReply, LogoutRequest, RegisterRequest,
    auth_service_server::{AuthService, AuthServiceServer},
};
use tonic::{Request, Response, Status};

pub struct Authenticator {}

#[tonic::async_trait]
impl AuthService for Authenticator {
    async fn register(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<AuthReply>, Status> {
        todo!()
    }
    async fn login(&self, request: Request<LoginRequest>) -> Result<Response<AuthReply>, Status> {
        todo!()
    }
    async fn logout(
        &self,
        request: Request<LogoutRequest>,
    ) -> Result<Response<LogoutReply>, Status> {
        todo!()
    }
}
