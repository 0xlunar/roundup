use std::fmt;
use std::fmt::Formatter;
use std::ops::Not;
use std::time::Duration;
use anyhow::format_err;
use async_trait::async_trait;
use log::{debug, error, warn};
use qbittorrent::queries::TorrentDownload;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::Instant;
use crate::api::imdb::{IMDBEpisode, ItemType};

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
            source
        }
    }
}

pub struct Torrenter {
    client: qbittorrent::Api,
    mpsc: UnboundedSender<String>,
    min_quality: MediaQuality,
    trackers: Vec<String>,
}
impl Torrenter {
    // Blocks until successful
    pub async fn new(
        username: &str,
        password: &str,
        address: &str,
        min_quality: MediaQuality,
        mpsc_sender: UnboundedSender<String>,
        trackers: Vec<String>,
    ) -> Self {
        let mut client = None;
        while client.is_none() {
            match qbittorrent::Api::new(username, password, address).await {
                Ok(c) => {
                    client = Some(c);
                    break;
                }
                Err(err) => {
                    error!("Waiting for qBittorrent to start...");
                    debug!("{}", err);
                    tokio::time::sleep_until(Instant::now() + Duration::from_secs(1)).await;
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
            let search_term = search_term.clone();
            let imdb_id = imdb_id.clone();
            let tv_episodes = tv_episodes.clone();
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
            let output = results.into_par_iter().filter_map(|task|
                match task {
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
            }).flatten().collect::<Vec<_>>();
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

        let torrent = TorrentDownload::new(Some(item.magnet_uri), None);
        self.client.add_new_torrent(&torrent).await?;
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
