use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use zohar_db::postgres_backend;
use zohar_protocol::token::TokenSigner;

#[derive(Debug, Parser)]
struct Cli {
    #[arg(long, default_value = "0.0.0.0:11000")]
    listen: String,
    #[arg(long, env = "ZOHAR_AUTH_DATABASE_URL")]
    auth_db_url: String,
    #[arg(long, env = "ZOHAR_AUTH_TOKEN_SECRET")]
    token_secret: String,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..), default_value_t = 30)]
    token_window_secs: u64,
    #[arg(long, default_value = "info,zohar_authsrv=info,zohar_db=info")]
    log_filter: String,
}

fn build_token_signer(cli: &Cli) -> Arc<TokenSigner> {
    let secret = cli.token_secret.as_bytes().to_vec();
    Arc::new(TokenSigner::new(
        secret,
        Duration::from_secs(cli.token_window_secs),
    ))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::registry()
        .with(EnvFilter::new(&cli.log_filter))
        .with(fmt::layer().with_timer(fmt::time::ChronoLocal::new("%H:%M:%S%.3f".into())))
        .init();

    let auth_db = postgres_backend::open_auth_db(&cli.auth_db_url)
        .await
        .context("open auth db")?;

    let token_signer = build_token_signer(&cli);

    zohar_authsrv::serve(cli.listen, auth_db, token_signer).await;

    Ok(())
}
