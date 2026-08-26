#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    rust_ai::run_cli().await
}
