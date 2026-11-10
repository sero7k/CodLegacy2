#![forbid(unsafe_code)]

#[tokio::main]
async fn main() -> codlegacy_master::Result<()> {
    codlegacy_master::cli_main().await
}
