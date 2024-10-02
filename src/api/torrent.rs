use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fmt::Formatter;
use std::ops::Not;
use std::sync::Arc;
use std::time::Duration;

use actix_web::web::Data;
use anyhow::format_err;
use async_trait::async_trait;
use chrono::{DateTime, Local};
use log::{debug, error, info, warn};
use qbittorrent::Api;
// use qbittorrent::data::{Hash, Torrent};
use qbittorrent::queries::TorrentDownload;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::Instant;

use crate::api::imdb::{IMDBEpisode, ItemType};
use crate::api::torrent_client::{Torrent, TorrentClient, TorrentFilePriority, TorrentState};
use crate::AppConfig;
use crate::db::DBConnection;
use crate::db::downloads::DownloadDatabase;

use super::torrent_client;

#[async_trait]
pub trait TorrentSearch: Send {
    async fn search(
        &self,
        search_term: String,
        imdb_id: Option<String>,
        tv_episodes: Option<Vec<IMDBEpisode>>,
    ) -> anyhow::Result<Vec<TorrentItem>>;
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Ord, PartialOrd, Copy, Clone)]
pub enum MediaQuality {
    #[serde(alias = "unknown")]
    Unknown,
    #[serde(alias = "cam")]
    Cam,
    #[serde(alias = "telesync", alias = "ts")]
    Telesync,
    #[serde(alias = "480p")]
    _480p,
    #[serde(alias = "720p")]
    _720p,
    #[serde(alias = "1080p")]
    _1080p,
    #[serde(alias = "betterThan1080p")]
    BetterThan1080p,
    #[serde(alias = "2160p", alias = "4k", alias = "4K")]
    _2160p, // 4K
    #[serde(alias = "4320p", alias = "8k", alias = "8K")]
    _4320p, // 8K
}

#[derive(Serialize, Debug)]
pub struct TorrentItem {
    pub imdb_id: String,
    pub name: String,
    pub magnet_uri: String,
    pub quality: MediaQuality,
    #[serde(rename = "type")]
    pub _type: ItemType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub season: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub episode: Option<i32>,
    pub seeds: Option<u32>,
    pub source: String,
}

impl TorrentItem {
    pub fn new(
        imdb_id: String,
        name: String,
        magnet_uri: String,
        quality: MediaQuality,
        _type: ItemType,
        season: Option<i32>,
        episode: Option<i32>,
        seeds: Option<u32>,
        source: String,
    ) -> Self {
        Self {
            imdb_id,
            name,
            magnet_uri,
            quality,
            _type,
            season,
            episode,
            seeds,
            source,
        }
    }
}

pub struct Torrenter {
    pub client: Box<dyn TorrentClient>,
    mpsc: UnboundedSender<String>,
    min_quality: MediaQuality,
    trackers: Vec<String>,
}
impl Torrenter {
    /**
    Waits until a connection is established.
     */
    pub async fn new(
        username: &str,
        password: &str,
        address: &str,
        min_quality: MediaQuality,
        mpsc_sender: UnboundedSender<String>,
        trackers: Vec<String>,
    ) -> Self {
        let username = if username.is_empty() {
            None
        } else {
            Some(username)
        };

        let password = if password.is_empty() {
            None
        } else {
            Some(password)
        };

        let mut client = None;
        while client.is_none() {
            let available_clients: Vec<Box<dyn TorrentClient>> = vec![
                Box::new(torrent_client::qbittorrent::QbittorrentWrapper::new()),
                Box::new(torrent_client::transmission::TransmissionWrapper::new()),
            ];
            for mut a_client in available_clients {
                match a_client.initialise(address, username, password).await {
                    Ok(_) => {
                        client = Some(a_client);
                        break;
                    }
                    Err(err) => {
                        warn!("Waiting for torrent client to start...");
                        debug!("{}", err);
                    }
                }
            }
        }

        let client = client.unwrap();

        Self {
            client,
            min_quality,
            mpsc: mpsc_sender,
            trackers,
        }
    }

    pub async fn find_torrent(
        &self,
        search_term: String,
        imdb_id: Option<String>,
        tv_episodes: Option<Vec<IMDBEpisode>>,
        concurrent_search: bool,
    ) -> anyhow::Result<Vec<TorrentItem>> {
        let ordering: Vec<Box<dyn TorrentSearch>> = vec![
            crate::api::yts::YTS::new(&self.trackers), // Movie
            crate::api::eztv::EZTV::new(),             // TV
            crate::api::therarbg::TheRARBG::new(),     // Any
        ];

        if concurrent_search {
            let tasks = ordering
                .into_iter()
                .map(|site| {
                    let search_term = search_term.clone();
                    let imdb_id = imdb_id.clone();
                    let tv_episodes = tv_episodes.clone();
                    (site, (search_term, imdb_id, tv_episodes))
                })
                .map(|(site, (search_term, imdb_id, tv_episodes))| async move {
                    site.search(search_term, imdb_id, tv_episodes).await
                })
                .collect::<Vec<_>>();

            let results = futures::future::join_all(tasks).await;
            let output = results
                .into_par_iter()
                .filter_map(|task| match task {
                    Ok(r) => {
                        if r.is_empty() {
                            None
                        } else {
                            let filtered = r
                                .into_par_iter()
                                .filter(|item| (item.quality as u8) >= (self.min_quality as u8))
                                .collect::<Vec<TorrentItem>>();
                            if filtered.is_empty().not() {
                                Some(filtered)
                            } else {
                                None
                            }
                        }
                    }
                    Err(e) => {
                        warn!("{}", e);
                        None
                    }
                })
                .flatten()
                .collect::<Vec<_>>();
            if output.is_empty().not() {
                return Ok(output);
            }
        } else {
            for site in ordering {
                match site
                    .search(search_term.clone(), imdb_id.clone(), tv_episodes.clone())
                    .await
                {
                    Ok(r) => {
                        if r.is_empty() {
                            continue;
                        } else {
                            let filtered = r
                                .into_par_iter()
                                .filter(|item| (item.quality as u8) >= (self.min_quality as u8))
                                .collect::<Vec<TorrentItem>>();
                            if filtered.is_empty().not() {
                                return Ok(filtered);
                            } else {
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("{}", e);
                        continue;
                    }
                };
            }
        }

        Err(format_err!("No torrents found matching criteria"))
    }

    pub async fn start_download(&self, item: TorrentItem) -> anyhow::Result<()> {
        let hash = item
            .magnet_uri
            .split_at(20)
            .1
            .split_once('&')
            .unwrap()
            .0
            .to_lowercase();
        self.mpsc.send(hash)?;

        // let torrent = TorrentDownload::new(Some(item.magnet_uri), None);
        self.client.add_torrent(&item.magnet_uri).await?;
        Ok(())
    }
}

impl fmt::Display for MediaQuality {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            MediaQuality::Unknown => write!(f, "Unknown"),
            MediaQuality::Cam => write!(f, "Cam"),
            MediaQuality::Telesync => write!(f, "Telesync"),
            MediaQuality::_720p => write!(f, "720p"),
            MediaQuality::_1080p => write!(f, "1080p"),
            MediaQuality::BetterThan1080p => write!(f, "Better than 1080p"),
            MediaQuality::_480p => write!(f, "480p"),
            MediaQuality::_2160p => write!(f, "2160p"),
            MediaQuality::_4320p => write!(f, "4320p"),
        }
    }
}

pub async fn monitor_torrents(
    client: Arc<Torrenter>,
    config: &Data<AppConfig>,
    db: &Data<DBConnection>,
    torrents_filtered: &mut HashSet<String>,
    stalled_torrents: &mut HashMap<String, (TorrentState, DateTime<Local>)>,
    auto_torrents: &mut HashSet<String>,
) {
    let client = &client.client;
    let torrents = match client.get_torrents().await {
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

    let hashes = torrents.par_iter().map(|x| &x.hash).collect::<Vec<_>>();
    let _ = db.remove_all_finished().await;
    let _ = db.remove_manually_removed(&hashes).await;

    let completed = torrents
        .iter()
        .filter(|t| matches!(t.state, TorrentState::Completed))
        .map(|t| {
            let hash = &t.hash;
            torrents_filtered.remove(hash);
            stalled_torrents.remove(hash);
            auto_torrents.remove(hash);
            hash.as_str()
        })
        .collect::<Vec<_>>();

    if !completed.is_empty() {
        match client.remove_torrents(completed).await {
            Ok(_) => {}
            Err(e) => error!("Error Deleting torrents: {}", e),
        }
    }

    // Updating Database items
    for torrent in torrents.iter() {
        let state = torrent.state.to_string();
        match db
            .update(&torrent.hash, state.as_str(), torrent.progress)
            .await
        {
            Ok(_) => (),
            Err(e) => error!("DB Error updating download: {}", e),
        }
    }

    // TODO: Find better way of doing this
    let filtered_clone = torrents_filtered.clone();
    let torrents = torrents.par_iter().filter(|t| {
        let hash = t.hash.as_str();
        let contains = auto_torrents.contains(hash);
        contains && filtered_clone.contains(hash).not()
    } && matches!(t.state, TorrentState::Downloading | TorrentState::Stalled)).collect::<Vec<_>>();

    let mut thirty_minutes_ago: DateTime<Local> = Local::now();
    thirty_minutes_ago = thirty_minutes_ago
        .checked_sub_signed(chrono::Duration::minutes(30))
        .unwrap();

    let mut torrents_to_reannounce = vec![];

    for torrent in torrents {
        stalled_torrents
            .entry(torrent.hash.clone())
            .and_modify(|(state, time)| {
                if *state != torrent.state {
                    *state = torrent.state.clone();
                    *time = Local::now();
                } else if *state == TorrentState::Stalled && *time <= thirty_minutes_ago {
                    *time = Local::now();
                    torrents_to_reannounce.push(torrent.hash.as_str());
                }
            })
            .or_insert((torrent.state.clone(), Local::now()));

        let contents = match &torrent.files {
            Some(files) => files,
            None => {
                continue;
            }
        };

        let mut files_to_remove: Vec<i64> = Vec::new();
        let valid_file_types = &config.valid_file_types;
        for content in contents {
            if !valid_file_types
                .iter()
                .any(|t| content.file_name.ends_with(t))
            {
                files_to_remove.push(content.id);
            }
        }

        if files_to_remove.is_empty() {
            continue;
        }

        match client
            .set_file_priority(
                &torrent.hash,
                files_to_remove,
                TorrentFilePriority::DisallowDownload,
            )
            .await
        {
            Ok(_) => {
                torrents_filtered.insert(torrent.hash.clone());
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
