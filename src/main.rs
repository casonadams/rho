#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    rho::run_cli().await
}
