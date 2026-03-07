#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    friday_backend::run_server("127.0.0.1:50051".parse()?).await
}
