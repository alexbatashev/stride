use crate::grpc::generated::friday::core::rpc::{
    HelloRequest, hello_service_client::HelloServiceClient,
};
use tonic::Request;
use tonic::transport::Channel;

pub async fn say_hello(endpoint: &str, name: &str) -> Result<String, tonic::Status> {
    let channel = Channel::from_shared(endpoint.to_string())
        .map_err(|e| tonic::Status::invalid_argument(e.to_string()))?
        .connect()
        .await
        .map_err(|e| tonic::Status::unavailable(e.to_string()))?;

    let mut client = HelloServiceClient::new(channel);
    let request = Request::new(HelloRequest {
        name: name.to_owned(),
    });
    let response = client.say_hello(request).await?;
    let message = response.into_inner().message;

    Ok(message)
}
