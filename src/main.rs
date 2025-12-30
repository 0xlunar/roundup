use crate::database::Database;
use crate::torrent::qbittorrent::{QBittorrent, QBittorrentCredentials};
use actix_web::middleware::Logger;
use actix_web::{App, HttpServer};
use config_updater::ConfigMonitor;
use log::LevelFilter;
use log4rs::{
    append::{console::ConsoleAppender, file::FileAppender},
    config::{Appender, Root},
    encode::pattern::PatternEncoder,
};
use serde::Deserialize;
use std::sync::Arc;

mod database;
mod managers;
mod scrapers;
mod server;
mod torrent;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let logfile = FileAppender::builder().build("log/output.log")?;

    let config = log4rs::Config::builder()
        .appenders(vec![
            Appender::builder().build(
                "stdout",
                Box::new(
                    ConsoleAppender::builder()
                        .encoder(Box::new(PatternEncoder::new(
                            "[{d(%s)} {h({l})} {t}] {m}{n}",
                        )))
                        .build(),
                ),
            ),
            Appender::builder().build("logfile", Box::new(logfile)),
        ])
        .build(
            Root::builder()
                .appenders(vec!["stdout", "logfile"])
                .build(LevelFilter::Info),
        )?;
    log4rs::init_config(config)?;

    let app_config_monitor: ConfigMonitor<AppConfig> = ConfigMonitor::new("./config.json", None);
    let app_config = app_config_monitor.data();
    let app_config_handle = app_config_monitor.monitor();

    let database = {
        let config = app_config.lock().await;
        Arc::new(Database::new(&config.database_url).await?)
    };

    let wreq_client = wreq::Client::new();

    let temp_config = {
        let lock = app_config.lock().await;
        lock.clone()
    };

    let qbittorrent = QBittorrent::new(
        wreq_client.clone(),
        temp_config.qbittorrent_url,
        QBittorrentCredentials::new(
            temp_config.qbittorrent_username,
            temp_config.qbittorrent_password,
        ),
    );
    let torrent_manager = managers::TorrentManager::connect(qbittorrent, database.clone()).await?;
    let (sender, torrent_manager_handle) = torrent_manager.start();

    let plex_manager = Arc::new(managers::PlexManager::new(
        wreq_client.clone(),
        database.clone(),
        temp_config.plex_base_url,
    )?);

    let _ = HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(app_config.clone())
            .app_data(sender.clone())
            .app_data(database.clone())
            .app_data(plex_manager.clone())
            .service(actix_files::Files::new("/static", "."))
            .service(server::api::index)
            .service(server::api::download)
            .service(server::api::search)
    })
    .bind(("0.0.0.0", 80))?
    .run()
    .await;

    let _ = torrent_manager_handle.await;
    let _ = app_config_handle.await;

    Ok(())
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    database_url: String,
    qbittorrent_url: String,
    qbittorrent_username: String,
    qbittorrent_password: String,
    plex_base_url: String,
}
