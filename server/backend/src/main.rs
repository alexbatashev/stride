#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    friday_backend::run_server("0.0.0.0:50051".parse()?, "0.0.0.0:8080".parse()?).await
}
