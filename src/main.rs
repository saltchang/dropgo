use std::net::SocketAddr;

use clap::Parser;
use dropgo::{
    Inner,
    config::{Cli, Command},
    lifecycle, store,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let cli = Cli::parse();
    let cfg = cli.config;
    let db = sqlx::postgres::PgPoolOptions::new().max_connections(10).connect(&cfg.database_url).await?;
    let store = store::from_url(&cfg.store_url)?;
    let st = Inner::new(cfg, db, store).await?;

    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => {
            tokio::spawn(lifecycle::spawn_job(st.clone()));
            let listener = tokio::net::TcpListener::bind(&st.cfg.listen).await?;
            tracing::info!("listening on {}", st.cfg.listen);
            let app = dropgo::router(st.clone()).into_make_service_with_connect_info::<SocketAddr>();
            axum::serve(listener, app).with_graceful_shutdown(shutdown()).await?;
            tracing::info!("waiting for uploads to finish");
            dropgo::drain(&st).await;
        }
        Command::Rerender => dropgo::rerender(&st).await?,
    }
    Ok(())
}

async fn shutdown() {
    let term = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler")
            .recv()
            .await;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term => {}
    }
}
