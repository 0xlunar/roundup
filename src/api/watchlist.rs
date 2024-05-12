use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use actix_web::web::Data;
use anyhow::format_err;
use chrono::Datelike;
use tokio::time::Instant;
use log::{info, warn};
use crate::api::imdb::{IMDBEpisode, IMDBItem, ItemType};
use crate::api::moviedb::MovieDBItem;
use crate::api::plex::Plex;
use crate::api::torrent::{MediaQuality, Torrenter, TorrentItem};
use crate::AppConfig;
use crate::db::DBConnection;
use crate::db::downloads::DownloadDatabase;
use crate::db::imdb::IMDBDatabase;
use crate::db::moviedb::MovieDBDatabase;
use crate::server::download;
use crate::server::download::TorrentQuery;

static TWELVE_HOURS: u64 = 43_200;

pub async fn monitor_watchlist(db: Arc<DBConnection>, plex: Arc<Plex>, torrenter: Arc<Torrenter>, app_config: Data<AppConfig>) {
    info!("Starting Watchlist Monitor");
    let imdb_db = IMDBDatabase::new(db.deref());
    let movie_db = MovieDBDatabase::new(db.deref());
    let recheck_delay = Duration::from_secs(TWELVE_HOURS);

    loop {
        info!("Fetching Watchlist");
        match app_config.tmdb_api_key.is_empty() {
            true => {
                let watchlist = imdb_db.fetch_watchlist().await.unwrap();
                if watchlist.is_empty() {
                    let _ = tokio::time::sleep_until(Instant::now() + recheck_delay).await;
                    continue;
                }

                info!("Checking downloads for Items");

                for item in watchlist {
                    info!("Checking: {} - {}", item.title, item.id);
                    let result= match item._type {
                        ItemType::Movie => check_movie_downloads_imdb(&item, torrenter.clone(), Arc::clone(&db), Data::clone(&app_config)).await,
                        ItemType::TvShow => check_tv_downloads_imdb(&item, plex.clone(), torrenter.clone(), Data::clone(&app_config), Arc::clone(&db)).await,
                    };

                    match result {
                        Ok(_) => (),
                        Err(e) => {
                            warn!("{}", e);
                            continue;
                        }
                    }
                }
            },
            false => {
                let watchlist = movie_db.fetch_watchlist().await.unwrap();
                if watchlist.is_empty() {
                    let _ = tokio::time::sleep_until(Instant::now() + recheck_delay).await;
                    continue;
                }

                info!("Checking downloads for Items");

                for item in watchlist {
                    info!("Checking: {} - {}", item.title, item.id);
                    let result= match item._type {
                        ItemType::Movie => check_movie_downloads_moviedb(&item, torrenter.clone(), Arc::clone(&db)).await,
                        ItemType::TvShow => check_tv_downloads_moviedb(&item, plex.clone(), torrenter.clone(), Data::clone(&app_config)).await,
                    };

                    match result {
                        Ok(_) => (),
                        Err(e) => {
                            warn!("{}", e);
                            continue;
                        }
                    }
                }
            },
        }
        
        info!("Sleeping for {} hours...", recheck_delay.as_secs() / 60 / 60);
        let _ = tokio::time::sleep_until(Instant::now() + recheck_delay).await;
    }
}

async fn check_movie_downloads_imdb(item: &IMDBItem, torrenter: Arc<Torrenter>, db: Arc<DBConnection>, _: Data<AppConfig>) -> anyhow::Result<()> {
    find_downloads_and_start_imdb(item, None, torrenter, db.clone()).await?;
    
    // Remove from watchlist as no further movies will release under this ID
    let imdb_db = IMDBDatabase::new(db.deref());
    imdb_db.update_watchlist_item(&item.id, false).await?;

    Ok(())
}
async fn check_tv_downloads_imdb(item: &IMDBItem, plex: Arc<Plex>, torrenter: Arc<Torrenter>, app_config: Data<AppConfig>, db: Arc<DBConnection>) -> anyhow::Result<()> {
    let title = format!("{} ({})", &item.title, item.year);

    let missing_episodes = download::find_missing_tv_shows(plex, app_config, &item.id, &title).await?;
    if missing_episodes.is_none() {
        return Err(format_err!("No missing episodes"));
    }
    find_downloads_and_start_imdb(item, missing_episodes, torrenter, db).await?;

    // Don't remove from watchlist as TV show may have future seasons/episodes

    Ok(())
}

async fn check_movie_downloads_moviedb(item: &MovieDBItem, torrenter: Arc<Torrenter>, db: Arc<DBConnection>) -> anyhow::Result<()> {
    find_downloads_and_start_moviedb(item, None, torrenter).await?;

    // Remove from watchlist as no further movies will release under this ID
    let movie_db = MovieDBDatabase::new(db.deref());
    movie_db.update_watchlist_item(item.id, false).await?;

    Ok(())
}

async fn check_tv_downloads_moviedb(item: &MovieDBItem, plex: Arc<Plex>, torrenter: Arc<Torrenter>, app_config: Data<AppConfig>) -> anyhow::Result<()> {
    let title = format!("{} ({})", &item.title, item.release_date.year());

    let id = item.id.to_string();
    info!("Checking TV Downloads: {}", id);
    let missing_episodes = download::find_missing_tv_shows(plex, app_config, &id, &title).await?;
    if missing_episodes.is_none() {
        return Err(format_err!("No missing episodes"));
    }
    find_downloads_and_start_moviedb(item, missing_episodes, torrenter).await?;

    // Don't remove from watchlist as TV show may have future seasons/episodes

    Ok(())
}


async fn find_downloads_and_start_imdb(item: &IMDBItem, episodes: Option<Vec<IMDBEpisode>>, torrenter: Arc<Torrenter>, db: Arc<DBConnection>) -> anyhow::Result<()> {
    let torrents = match torrenter.find_torrent(item.title.to_owned(), Some(item.id.to_owned()), episodes).await {
        Ok(t) => t,
        Err(e) => return Err(e),
    };

    let torrents = torrents.into_iter().filter(|x| x.quality == MediaQuality::_1080p).collect::<Vec<TorrentItem>>();
    if torrents.is_empty() {
        return Err(format_err!("No torrents available"));
    }
    let download_db = DownloadDatabase::new(db.deref());
    info!("Downloading Item: {}", item.id);
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
async fn find_downloads_and_start_moviedb(item: &MovieDBItem, episodes: Option<Vec<IMDBEpisode>>, torrenter: Arc<Torrenter>) -> anyhow::Result<()> {
    let torrents = match torrenter.find_torrent(item.title.to_owned(), Some(item.imdb_id.to_owned()), episodes).await {
        Ok(t) => t,
        Err(e) => return Err(e),
    };

    let torrents = torrents.into_iter().filter(|x| x.quality == MediaQuality::_1080p).collect::<Vec<TorrentItem>>();
    if torrents.is_empty() {
        return Err(format_err!("No torrents available"));
    }

    info!("Downloading Item: {}", item.id);
    for torrent in torrents {
        match torrenter.start_download(torrent).await {
            Ok(_) => (),
            Err(e) => return Err(format_err!("Failed to start download, {}", e)),
        };
    }

    Ok(())
}