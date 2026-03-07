use crate::grpc::generated::friday::core::rpc::{
    AuthReply, LoginRequest, LogoutReply, LogoutRequest, RegisterRequest,
    auth_service_client::AuthServiceClient,
};
use tonic::Request;
use tonic::transport::Channel;

#[derive(Debug, Clone)]
pub struct AuthSession {
    pub token: String,
    pub user_id: String,
    pub expires_at: i64,
}

async fn connect(endpoint: &str) -> Result<AuthServiceClient<Channel>, tonic::Status> {
    let channel = Channel::from_shared(endpoint.to_string())
        .map_err(|e| tonic::Status::invalid_argument(e.to_string()))?
        .connect()
        .await
        .map_err(|e| tonic::Status::unavailable(e.to_string()))?;
    Ok(AuthServiceClient::new(channel))
}

fn into_session(reply: AuthReply) -> AuthSession {
    AuthSession {
        token: reply.token,
        user_id: reply.user_id,
        expires_at: reply.expires_at,
    }
}

pub async fn register(
    endpoint: &str,
    email: &str,
    password: &str,
) -> Result<AuthSession, tonic::Status> {
    let mut client = connect(endpoint).await?;
    let response = client
        .register(Request::new(RegisterRequest {
            email: email.to_owned(),
            password: password.to_owned(),
        }))
        .await?;
    Ok(into_session(response.into_inner()))
}

pub async fn login(
    endpoint: &str,
    email: &str,
    password: &str,
) -> Result<AuthSession, tonic::Status> {
    let mut client = connect(endpoint).await?;
    let response = client
        .login(Request::new(LoginRequest {
            email: email.to_owned(),
            password: password.to_owned(),
        }))
        .await?;
    Ok(into_session(response.into_inner()))
}

pub async fn logout(endpoint: &str, token: &str) -> Result<bool, tonic::Status> {
    let mut client = connect(endpoint).await?;
    let response = client
        .logout(Request::new(LogoutRequest {
            token: token.to_owned(),
        }))
        .await?;
    let LogoutReply { success } = response.into_inner();
    Ok(success)
}
