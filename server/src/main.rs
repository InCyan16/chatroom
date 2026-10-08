mod commands;
mod config;
mod engine;
mod web;

use config::Config;
use engine::{Engine, hash_password};
use std::{
    collections::HashMap,
    io::{self, BufRead},
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use web::App;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(io::stdout)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "chat=info,tower_http=info".into()),
        )
        .init();
    let data_dir = std::env::var("CHAT_DATA").unwrap_or_else(|_| "data".into());
    std::fs::create_dir_all(&data_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut engine = Engine::open(&Path::new(&data_dir).join("chat.sqlite"))?;
    let config = Config::load(Path::new(&data_dir))?;
    engine.set_limits(config.max_users, config.max_rooms, config.max_messages)?;
    if engine.data.users.is_empty() {
        tracing::info!("No accounts yet. Console: /user alice a-long-password admin");
    }
    let (changes, _) = broadcast::channel(16);
    let app = App {
        engine: Arc::new(Mutex::new(engine)),
        attempts: Arc::new(Mutex::new(HashMap::new())),
        changes,
        config: Arc::new(config),
        connections: Arc::new(tokio::sync::Semaphore::new(128)),
        password_jobs: Arc::new(tokio::sync::Semaphore::new(2)),
        stopping: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let listener = tokio::net::TcpListener::bind(&app.config.bind).await?;
    console(app.clone());
    tracing::info!(address = %listener.local_addr()?, production = app.config.production, "Chat server ready");
    axum::serve(
        listener,
        web::router(app.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown({
        let app = app.clone();
        async move {
            shutdown_signal().await;
            app.stopping
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = app.changes.send(());
            tracing::info!("Shutting down");
        }
    })
    .await?;
    app.engine.lock().unwrap().checkpoint()?;
    Ok(())
}
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Install SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn console(app: App) {
    std::thread::spawn(move || {
        tracing::info!("Console ready. /help lists commands. Console input is never logged.");
        for line in io::stdin().lock().lines() {
            let line = match line {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!(error = %e, "Console read failed");
                    break;
                }
            };
            let parts: Vec<_> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }
            let before = app.engine.lock().unwrap().revision;
            let result = match parts[0] {
                "/help" => Ok(commands::help(true, true)),
                "/configs" if parts.len() == 1 => app.config.display(),
                "/configs" => Err("Usage: /configs".into()),
                "/user" | "/reset" => {
                    let reset = parts[0] == "/reset";
                    if (reset && parts.len() != 3) || (!reset && !(3..=4).contains(&parts.len())) {
                        Err(
                            "Usage: /user name password [admin|user] or /reset name password"
                                .into(),
                        )
                    } else if !reset
                        && parts
                            .get(3)
                            .is_some_and(|role| !["admin", "user"].contains(role))
                    {
                        Err("Role must be admin or user.".into())
                    } else {
                        hash_password(parts[2]).and_then(|hash| {
                            app.engine.lock().unwrap().provision(
                                parts[1],
                                hash,
                                parts.get(3) == Some(&"admin"),
                                reset,
                            )
                        })
                    }
                }
                _ => app.engine.lock().unwrap().execute(None, None, &line),
            };
            match result {
                Ok(reply) => {
                    tracing::info!("{reply}");
                    if app.engine.lock().unwrap().revision != before {
                        let _ = app.changes.send(());
                    }
                }
                Err(error) => tracing::warn!("{error}"),
            }
        }
        tracing::info!("Console stdin closed; web server remains running.");
    });
}
