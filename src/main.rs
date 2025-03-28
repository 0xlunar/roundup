use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::BufReader;
use std::ops::Not;
use std::sync::Arc;
use std::time::Duration;

use actix_web::middleware::Logger;
use actix_web::web::Data;
use actix_web::{App, HttpServer};
use chrono::{DateTime, Local};
use log::{error, info};
use qbittorrent::data::{Hash, State, Torrent};
use qbittorrent::traits::TorrentData;
use qbittorrent::Api;
use rayon::prelude::*;
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::api::imdb::SearchType;
use crate::api::torrent::MediaQuality;
use crate::db::downloads::DownloadDatabase;
use crate::db::initialiser::DatabaseInitialiser;
use crate::db::DBConnection;

mod api;
mod db;
mod server;

pub type QueryCache = Vec<(SearchType, DateTime<Local>)>;

///
/// Environment Variables
/// key: type
///
/// QBITTORRENT_URL: String
/// QBITTORRENT_USERNAME: String
/// QBITTORRENT_PASSWORD: String
/// DB_URL: String
/// DB_URI: String - Alternative to DB_URL
/// MINIMUM_QUALITY: String
/// YOUTUBE_API_KEY: String
/// WATCHLIST_RECHECK_INTERVAL_HOURS: i64
/// CONCURRENT_TORRENT_SEARCH: bool
/// PLEX_URL: String
///
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    if cfg!(debug_assertions) {
        console_subscriber::init();
    }

    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(_) => {
                info!("Exiting...");
                std::process::exit(0);
            }
            Err(err) => {
                error!("Failed to handle signal: {}", err);
                std::process::exit(1);
            }
        }
    });

    let config = AppConfig::load();

    // This is to trigger a fresh check on launch for first time of request type
    let mut twelve_hour_ago: DateTime<Local> = Local::now();
    twelve_hour_ago = twelve_hour_ago
        .checked_sub_signed(chrono::Duration::hours(12))
        .unwrap();

    let plex_session = api::plex::Plex::new()?;

    let (torrent_tx, mut torrent_rx) = tokio::sync::mpsc::unbounded_channel();
    let torrent_client = api::torrent::Torrenter::new(
        &config.qbittorrent_username,
        &config.qbittorrent_password,
        &config.qbittorrent_url,
        config.minimum_quality,
        torrent_tx.clone(),
        config.trackers.clone(),
        config.proxy.clone()
    )
    .await;

    let db_conn = match config.db_url.is_empty() {
        true => DBConnection::from_env("DB_URI").await?,
        false => DBConnection::new(&config.db_url).await?,
    };

    match DatabaseInitialiser::new(&db_conn).initialise().await {
        Ok(_) => info!("Initialised Database"),
        Err(e) => {
            panic!("Error Initialising DB: {}", e);
        }
    };

    let cache_update: QueryCache = vec![
        (SearchType::MoviePopular, twelve_hour_ago.to_owned()),
        (SearchType::MovieLatestRelease, twelve_hour_ago.to_owned()),
        (SearchType::TVPopular, twelve_hour_ago.to_owned()),
        (SearchType::TVLatestRelease, twelve_hour_ago),
    ];

    let youtube = api::youtube::Youtube::new(&config.youtube_api_key, config.proxy.as_ref());
    let db_conn = Data::new(db_conn);
    let app_config = Data::new(config);

    let app_config_clone = Data::clone(&app_config);
    let db = Data::clone(&db_conn);
    let torrent_watcher = tokio::task::spawn(async move {
        let config = Data::clone(&app_config_clone);
        let delay_dur = Duration::from_millis(15000);
        let client = Api::new(
            &config.qbittorrent_username,
            &config.qbittorrent_password,
            &config.qbittorrent_url,
        )
        .await
        .unwrap();
        let mut torrents_filtered = HashSet::new();
        let mut stalled_torrents = HashMap::new();
        let mut auto_torrents = HashSet::new();
        loop {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            while let Some(val) = tokio::select! {
                Some(val) = torrent_rx.recv() => {
                    Some(val)
                }
                _ = interval.tick() => None
            } {
                auto_torrents.insert(val);
            }
            let _ = monitor_torrents(
                &client,
                &config,
                &db,
                &mut torrents_filtered,
                &mut stalled_torrents,
                &mut auto_torrents,
            )
            .await;
            tokio::time::sleep_until(Instant::now() + delay_dur).await;
        }
    });

    let db_conn = Data::clone(&db_conn);
    let db_conn_watchlist = Data::clone(&db_conn);
    let torrent_client = Arc::new(torrent_client);
    let watchlist_task = tokio::task::spawn(api::watchlist::monitor_watchlist(
        db_conn_watchlist.into_inner(),
        Arc::new(plex_session.clone()),
        Arc::clone(&torrent_client),
        Data::clone(&app_config),
    ));

    let youtube = Data::new(youtube);
    let cache_update = Data::new(Mutex::new(cache_update));
    let plex_session = Data::new(plex_session);
    let torrent_client = Data::from(torrent_client);

    let server = HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(Data::clone(&cache_update))
            .app_data(Data::clone(&db_conn))
            .app_data(Data::clone(&plex_session))
            .app_data(Data::clone(&torrent_client))
            .app_data(Data::clone(&youtube))
            .app_data(Data::clone(&app_config))
            .service(actix_files::Files::new("/static", "./static").show_files_listing())
            .service(server::index)
            .service(server::query::search)
            .service(server::query::modal_metadata)
            .service(server::download::update_watchlist)
            .service(server::download::start_download)
            .service(server::download::find_download)
            .service(server::download::start_download_post)
    })
    .bind(("0.0.0.0", 80))?;

    let certs_file = File::open("./cert.pem");
    let key_file = File::open("./key.pem");
    let server = if certs_file.is_ok() && key_file.is_ok() {
        let mut certs_file = BufReader::new(certs_file.unwrap());
        let mut key_file = BufReader::new(key_file.unwrap());

        let tls_certs = rustls_pemfile::certs(&mut certs_file)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        let tls_key = rustls_pemfile::pkcs8_private_keys(&mut key_file)
            .next()
            .unwrap()
            .unwrap();

        let tls_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(tls_certs, rustls::pki_types::PrivateKeyDer::Pkcs8(tls_key))
            .unwrap();
        server.bind_rustls_0_22(("0.0.0.0", 443), tls_config)?
    } else {
        server
    };

    server.run().await?;

    watchlist_task.await?;
    torrent_watcher.await?;

    Ok(())
}

#[derive(Debug, Deserialize, Clone)]
struct AppConfigImport {
    qbittorrent_url: String,
    qbittorrent_username: String,
    qbittorrent_password: String,
    db_url: String,
    valid_file_types: Vec<String>,
    minimum_quality: String,
    youtube_api_key: String,
    watchlist_recheck_interval_hours: i64,
    #[serde(default)]
    trackers: Vec<String>,
    concurrent_torrent_search: bool,
    proxy: Option<String>,
}

#[derive(Debug, Clone)]
struct AppConfig {
    qbittorrent_url: String,
    qbittorrent_username: String,
    qbittorrent_password: String,
    db_url: String,
    valid_file_types: Vec<String>,
    minimum_quality: MediaQuality,
    youtube_api_key: String,
    watchlist_recheck_interval_hours: i64,
    trackers: Vec<String>,
    concurrent_torrent_search: bool,
    proxy: Option<String>,
}

impl AppConfig {
    pub fn load() -> AppConfig {
        let qbittorrent_url = std::env::var("QBITTORRENT_URL").ok();
        let qbittorrent_username = std::env::var("QBITTORRENT_USERNAME").ok();
        let qbittorrent_password = std::env::var("QBITTORRENT_PASSWORD").ok();
        let db_url = std::env::var("DB_URL").ok();
        let minimum_quality = std::env::var("MINIMUM_QUALITY").ok();
        let youtube_api_key = std::env::var("YOUTUBE_API_KEY").ok();
        let watchlist_recheck_interval_hours =
            std::env::var("WATCHLIST_RECHECK_INTERVAL_HOURS").ok();
        let watchlist_recheck_interval_hours = match watchlist_recheck_interval_hours {
            Some(hours) => hours.parse::<i64>().ok(),
            None => None,
        };

        let concurrent_torrent_search = std::env::var("CONCURRENT_TORRENT_SEARCH").ok();
        let concurrent_torrent_search = match concurrent_torrent_search {
            Some(hours) => hours.parse::<bool>().ok(),
            None => None,
        };

        let proxy = std::env::var("PROXY").ok();

        let buffer = match fs::read_to_string("./config.json") {
            Ok(buffer) => buffer,
            Err(err) => panic!("Unable to load ./config.json: {}", err),
        };

        let imported: AppConfigImport = serde_json::from_str(&buffer).unwrap();

        let minimum_quality = minimum_quality.unwrap_or(imported.minimum_quality);

        let config = AppConfig {
            qbittorrent_url: qbittorrent_url.unwrap_or(imported.qbittorrent_url),
            qbittorrent_username: qbittorrent_username.unwrap_or(imported.qbittorrent_username),
            qbittorrent_password: qbittorrent_password.unwrap_or(imported.qbittorrent_password),
            db_url: db_url.unwrap_or(imported.db_url),
            valid_file_types: imported.valid_file_types,
            minimum_quality: match minimum_quality.as_str() {
                "cam" => MediaQuality::Cam,
                "telesync" | "ts" | "tele-sync" => MediaQuality::Telesync,
                "720p" | "720" => MediaQuality::_720p,
                "1080p" | "1080" => MediaQuality::_1080p,
                "2160p" | "2160" | "4k" => MediaQuality::_2160p,
                "4320p" | "4320" | "8K" => MediaQuality::_4320p,
                _ => MediaQuality::Unknown,
            },
            youtube_api_key: youtube_api_key.unwrap_or(imported.youtube_api_key),
            watchlist_recheck_interval_hours: watchlist_recheck_interval_hours
                .unwrap_or(imported.watchlist_recheck_interval_hours),
            trackers: imported.trackers,
            concurrent_torrent_search: concurrent_torrent_search
                .unwrap_or(imported.concurrent_torrent_search),
            proxy: proxy.or(imported.proxy),
        };

        config
    }
}

async fn monitor_torrents(
    client: &Api,
    config: &Data<AppConfig>,
    db: &Data<DBConnection>,
    torrents_filtered: &mut HashSet<String>,
    stalled_torrents: &mut HashMap<String, (State, DateTime<Local>)>,
    auto_torrents: &mut HashSet<String>,
) {
    let torrents = match client.get_torrent_list().await {
        Ok(t) => t,
        Err(_) => {
            return;
        }
    };

    let db = DownloadDatabase::new(db);
    if torrents.is_empty() {
        let _ = db.remove_all().await;
        return;
    }

    let hashes = torrents
        .par_iter()
        .map(|x| x.hash())
        .collect::<Vec<&Hash>>();
    let _ = db.remove_all_finished().await;
    let _ = db.remove_manually_removed(&hashes).await;

    let completed = torrents
        .iter()
        .filter(|t| matches!(t.state(), State::PausedUP))
        .map(|t| {
            let hash = t.hash().clone().inner();
            torrents_filtered.remove(&hash);
            stalled_torrents.remove(&hash);
            auto_torrents.remove(t.magnet_uri());
            t.hash()
        })
        .collect::<Vec<&Hash>>();

    if !completed.is_empty() {
        match client.delete_torrents(completed, false).await {
            Ok(_) => {}
            Err(e) => error!("Error Deleting torrents: {}", e),
        }
    }

    // Updating Database items
    for torrent in torrents.iter() {
        match db
            .update(
                torrent.hash().as_str(),
                torrent.state().as_ref(),
                *torrent.progress(),
            )
            .await
        {
            Ok(_) => (),
            Err(e) => error!("DB Error updating download: {}", e),
        }
    }

    // TODO: Find better way of doing this
    let filtered_clone = torrents_filtered.clone();
    let torrents = torrents.par_iter().filter(|t| {
        let hash = t.hash().as_str();
        let contains = auto_torrents.contains(hash);
        contains && filtered_clone.contains(hash).not()
    } && matches!(t.state(), State::Downloading | State::StalledDL | State::ForceDL)).collect::<Vec<&Torrent>>();

    let mut thirty_minutes_ago: DateTime<Local> = Local::now();
    thirty_minutes_ago = thirty_minutes_ago
        .checked_sub_signed(chrono::Duration::minutes(30))
        .unwrap();

    let mut torrents_to_reannounce = vec![];

    for torrent in torrents {
        stalled_torrents
            .entry(torrent.hash().clone().inner())
            .and_modify(|(state, time)| {
                if *state != *torrent.state() {
                    *state = torrent.state().clone();
                    *time = Local::now();
                } else if *state == State::StalledDL && *time <= thirty_minutes_ago {
                    *time = Local::now();
                    torrents_to_reannounce.push(torrent.hash());
                }
            })
            .or_insert((torrent.state().clone(), Local::now()));

        let contents = match client.contents(torrent).await {
            Ok(c) => c,
            Err(_) => {
                continue;
            }
        };

        let mut files_to_remove: Vec<i64> = Vec::new();
        let valid_file_types = &config.valid_file_types;
        for content in contents {
            if !valid_file_types.iter().any(|t| content.name().ends_with(t)) {
                files_to_remove.push(*content.index());
            }
        }

        if files_to_remove.is_empty() {
            continue;
        }

        match client
            .set_file_priority(torrent.hash(), files_to_remove, 0)
            .await
        {
            Ok(_) => {
                torrents_filtered.insert(torrent.hash().clone().inner());
            }
            Err(e) => error!("Error: {}", e),
        }
    }

    if !torrents_to_reannounce.is_empty() {
        match client.reannounce_torrents(torrents_to_reannounce).await {
            Ok(_) => info!("Reannounced some torrents"),
            Err(e) => error!("Failed to reannounce torrents: {}", e),
        }
    }
}
