use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

use actix_web::web::Data;
use anyhow::format_err;
use chrono::Datelike;
use log::{info, warn};
use rayon::prelude::*;
use tokio::time::Instant;

use crate::api::imdb::{IMDBEpisode, IMDBItem, ItemType};
use crate::api::plex::Plex;
use crate::api::torrent::{MediaQuality, Torrenter, TorrentItem};
use crate::AppConfig;
use crate::db::DBConnection;
use crate::db::downloads::DownloadDatabase;
use crate::db::imdb::IMDBDatabase;
use crate::server::download;
use crate::server::download::TorrentQuery;

static ONE_HOUR: u64 = 3_600;

pub async fn monitor_watchlist(
    db: Arc<DBConnection>,
    plex: Arc<Plex>,
    torrenter: Arc<Torrenter>,
    app_config: Data<AppConfig>,
) {
    info!("Starting Watchlist Monitor");
    let imdb_db = IMDBDatabase::new(db.deref());
    let mut recheck_interval = 6;
    if app_config.watchlist_recheck_interval_hours.gt(&6) {
        // Minimum of 6 hours delay, to prevent pointless spam.
        recheck_interval = app_config.watchlist_recheck_interval_hours as u64;
    }
    let recheck_delay = Duration::from_secs(ONE_HOUR * recheck_interval);

    loop {
        info!("Fetching Watchlist");
        let watchlist = imdb_db.fetch_watchlist().await.unwrap();
        if watchlist.is_empty() {
            let _ = tokio::time::sleep_until(Instant::now() + recheck_delay).await;
            continue;
        }

        info!("Checking downloads for Items");

        for item in watchlist {
            info!("Checking: {} - {}", item.title, item.id);
            let result = match item._type {
                ItemType::Movie => {
                    check_movie_downloads_imdb(
                        &item,
                        torrenter.clone(),
                        Arc::clone(&db),
                        Data::clone(&app_config),
                    )
                    .await
                }
                ItemType::TvShow => {
                    check_tv_downloads_imdb(
                        &item,
                        plex.clone(),
                        torrenter.clone(),
                        Data::clone(&app_config),
                        Arc::clone(&db),
                    )
                    .await
                }
            };

            match result {
                Ok(_) => (),
                Err(e) => {
                    warn!("{}", e);
                    continue;
                }
            }
        }

        info!(
            "Sleeping for {} hours...",
            recheck_delay.as_secs() / 60 / 60
        );
        let _ = tokio::time::sleep_until(Instant::now() + recheck_delay).await;
    }
}

async fn check_movie_downloads_imdb(
    item: &IMDBItem,
    torrenter: Arc<Torrenter>,
    db: Arc<DBConnection>,
    config: Data<AppConfig>,
) -> anyhow::Result<()> {
    find_downloads_and_start_imdb(item, None, torrenter, db.clone(), config.concurrent_torrent_search).await?;

    // Remove from watchlist as no further movies will release under this ID
    let imdb_db = IMDBDatabase::new(db.deref());
    imdb_db.update_watchlist_item(&item.id, false).await?;

    Ok(())
}
async fn check_tv_downloads_imdb(
    item: &IMDBItem,
    plex: Arc<Plex>,
    torrenter: Arc<Torrenter>,
    app_config: Data<AppConfig>,
    db: Arc<DBConnection>,
) -> anyhow::Result<()> {
    let title = format!("{} ({})", &item.title, item.year);

    let concurrent_search = app_config.concurrent_torrent_search;
    let missing_episodes =
        download::find_missing_tv_shows(plex, &item.id, &title).await?;
    if missing_episodes.is_none() {
        return Err(format_err!("No missing episodes"));
    }
    find_downloads_and_start_imdb(item, missing_episodes, torrenter, db, concurrent_search).await?;

    // Don't remove from watchlist as TV show may have future seasons/episodes

    Ok(())
}

async fn find_downloads_and_start_imdb(
    item: &IMDBItem,
    episodes: Option<Vec<IMDBEpisode>>,
    torrenter: Arc<Torrenter>,
    db: Arc<DBConnection>,
    concurrent_search: bool,
) -> anyhow::Result<()> {
    let download_db = DownloadDatabase::new(db.deref());
    let (is_downloading, remaining_episodes) =
        download_db.is_downloading(&item.id, episodes).await?;

    if is_downloading && remaining_episodes.is_none() {
        return Err(format_err!("Already downloading."));
    }

    let torrents = match torrenter
        .find_torrent(
            item.title.to_owned(),
            Some(item.id.to_owned()),
            remaining_episodes,
            concurrent_search,
        )
        .await
    {
        Ok(t) => t,
        Err(e) => return Err(e),
    };

    let torrents = torrents
        .into_par_iter()
        .filter(|x| {
            x.quality == MediaQuality::_1080p
                && match x.episode {
                    Some(e) => e >= 0,
                    None => true,
                }
        })
        .collect::<Vec<TorrentItem>>();
    if torrents.is_empty() {
        return Err(format_err!("No torrents available"));
    }
    info!("Downloading Item: {}", item.id);
    let download_db = DownloadDatabase::new(db.deref());
    for torrent in torrents {
        let query = TorrentQuery {
            imdb_id: torrent.imdb_id.clone(),
            season: torrent.season,
            episode: torrent.episode,
            quality: torrent.quality,
            magnet_uri: torrent.magnet_uri.clone(),
        };

        match download_db.insert(&query).await {
            Ok(_) => (),
            Err(e) => return Err(format_err!("Failed to insert torrent, {}", e)),
        }
        match torrenter.start_download(torrent).await {
            Ok(_) => (),
            Err(e) => return Err(format_err!("Failed to start download, {}", e)),
        };
    }

    Ok(())
}
