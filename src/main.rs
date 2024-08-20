use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::BufReader;
use std::ops::Not;
use std::sync::Arc;
use std::time::Duration;

use actix_session::SessionMiddleware;
use actix_web::{App, HttpServer};
use actix_web::cookie::Key;
use actix_web::middleware::Logger;
use actix_web::web::Data;
use chrono::{DateTime, Local};
use log::{error, info, LevelFilter, warn};
use log4rs::append::console::ConsoleAppender;
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Root};
use log4rs::Config;
use log4rs::encode::pattern::PatternEncoder;
use oauth2::{
    AuthUrl, Client, ClientId, ClientSecret, EndpointNotSet, EndpointSet, RedirectUrl,
    RevocationUrl, StandardRevocableToken, TokenUrl,
};
use oauth2::basic::{
    BasicClient, BasicErrorResponse, BasicRevocationErrorResponse, BasicTokenIntrospectionResponse,
    BasicTokenResponse,
};
use qbittorrent::Api;
use qbittorrent::data::{Hash, State, Torrent};
use qbittorrent::traits::TorrentData;
use rayon::prelude::*;
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::api::imdb::SearchType;
use crate::api::torrent::MediaQuality;
use crate::db::DBConnection;
use crate::db::downloads::DownloadDatabase;
use crate::db::initialiser::DatabaseInitialiser;
use crate::db::session_psql::PostgreSQLSessionStore;

mod api;
mod db;
mod server;

pub type QueryCache = Vec<(SearchType, DateTime<Local>)>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let logfile = FileAppender::builder().build("log/output.log")?;

    let config = Config::builder()
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

    if cfg!(debug_assertions) {
        console_subscriber::init();
    }
    let mut config = AppConfig::load();

    match config.tmdb_api_key.is_empty() {
        true => info!("Using IMDB"),
        false => info!("Using The MovieDB"),
    };

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

    let session_store = match config.db_url.is_empty() {
        true => PostgreSQLSessionStore::from_env("DB_URI").await?,
        false => PostgreSQLSessionStore::new(&config.db_url).await?,
    };

    let cache_update: QueryCache = vec![
        (SearchType::MoviePopular, twelve_hour_ago.to_owned()),
        (SearchType::MovieLatestRelease, twelve_hour_ago.to_owned()),
        (SearchType::TVPopular, twelve_hour_ago.to_owned()),
        (SearchType::TVLatestRelease, twelve_hour_ago),
    ];

    let youtube = api::youtube::Youtube::new(&config.youtube_api_key);
    let db_conn = Data::new(db_conn);

    let google_client_id = config.google_client_id;
    config.google_client_id = None;
    let google_client_secret = config.google_client_secret;
    config.google_client_secret = None;
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

    let app_secret = Key::generate();

    let oauth_client = if google_client_id.is_some() && google_client_secret.is_some() {
        info!("Authentication Enabled");
        let id = google_client_id.unwrap();
        let google_client_id = ClientId::new(id);
        let secret = google_client_secret.unwrap();
        let google_client_secret = ClientSecret::new(secret);
        let auth_url = AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
            .expect("Invalid authorisation endpoint url");
        let token_url = TokenUrl::new("https://www.googleapis.com/oauth2/v3/token".to_string())
            .expect("Invalid token endpoint url");
        let redirect_url = RedirectUrl::new("https://localhost/auth_callback".to_string())
            .expect("Invalid redirect endpoint url");
        let revocation_url = RevocationUrl::new("https://oauth2.googleapis.com/revoke".to_string())
            .expect("Invalid revocation endpoint url");

        let client: Client<
            BasicErrorResponse,
            BasicTokenResponse,
            BasicTokenIntrospectionResponse,
            StandardRevocableToken,
            BasicRevocationErrorResponse,
            EndpointSet,
            EndpointNotSet,
            EndpointNotSet,
            EndpointSet,
            EndpointSet,
        > = BasicClient::new(google_client_id)
            .set_client_secret(google_client_secret)
            .set_auth_uri(auth_url)
            .set_token_uri(token_url)
            .set_redirect_uri(redirect_url)
            .set_revocation_url(revocation_url);

        Data::new(Some(client))
    } else {
        warn!("Authentication Disabled, supply a Google Client Id and Secret in the config to enable.");

        drop(google_client_id);
        drop(google_client_secret);
        Data::new(None)
    };

    let server = HttpServer::new(move || {
        App::new()
            .wrap(server::middleware::has_auth::HasAuthorisation)
            .wrap(
                SessionMiddleware::builder(session_store.clone(), app_secret.clone())
                    .cookie_name("roundup".to_string())
                    .cookie_secure(true)
                    .build(),
            )
            .wrap(Logger::default())
            .app_data(Data::clone(&oauth_client))
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
            .service(server::auth::auth_get)
            .service(server::auth::auth_callback)
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
struct AppConfig {
    qbittorrent_url: String,
    qbittorrent_username: String,
    qbittorrent_password: String,
    db_url: String,
    valid_file_types: Vec<String>,
    minimum_quality: MediaQuality,
    youtube_api_key: String,
    tmdb_api_key: String,
    watchlist_recheck_interval_hours: i64,
    trackers: Vec<String>,
    google_client_id: Option<String>,
    google_client_secret: Option<String>,
}

impl AppConfig {
    pub fn load() -> AppConfig {
        let buffer = fs::read_to_string("./config.json").unwrap();
        serde_json::from_str(&buffer).unwrap()
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
