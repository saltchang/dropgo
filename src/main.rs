use std::net::SocketAddr;

use dropgo::{
    Inner,
    config::{Cli, Command},
    lifecycle, store,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (cfg, serve) = match Cli::parse_args(std::env::args_os().collect()).command {
        Command::Client(c) => std::process::exit(dropgo::cli::run(c).await),
        Command::Serve(cfg) => (cfg, true),
        Command::Rerender(cfg) => (cfg, false),
    };
    tracing_subscriber::fmt().init();
    let db = sqlx::postgres::PgPoolOptions::new().max_connections(10).connect(&cfg.database_url).await?;
    let store = store::from_url(&cfg.store_url)?;
    let st = Inner::new(cfg, db, store).await?;

    if serve {
        tokio::spawn(lifecycle::spawn_job(st.clone()));
        let listener = tokio::net::TcpListener::bind(&st.cfg.listen).await?;
        tracing::info!("listening on {}", st.cfg.listen);
        let app = dropgo::router(st.clone()).into_make_service_with_connect_info::<SocketAddr>();
        axum::serve(listener, app).with_graceful_shutdown(shutdown()).await?;
        tracing::info!("waiting for uploads to finish");
        dropgo::drain(&st).await;
    } else {
        dropgo::rerender(&st).await?;
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
