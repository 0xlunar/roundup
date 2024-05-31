use std::fmt;
use std::fmt::Formatter;
use std::ops::Not;

use anyhow::format_err;
use async_trait::async_trait;
use log::warn;
use qbittorrent::queries::TorrentDownload;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;
use rayon::prelude::*;

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
        }
    }
}

pub struct Torrenter {
    client: qbittorrent::Api,
    mpsc: UnboundedSender<String>,
    min_quality: MediaQuality,
}
impl Torrenter {
    pub async fn new(
        username: &str,
        password: &str,
        address: &str,
        min_quality: MediaQuality,
        mpsc_sender: UnboundedSender<String>,
    ) -> Self {
        let client = qbittorrent::Api::new(username, password, address)
            .await
            .unwrap();

        Self {
            client,
            min_quality,
            mpsc: mpsc_sender,
        }
    }

    pub async fn find_torrent(
        &self,
        search_term: String,
        imdb_id: Option<String>,
        tv_episodes: Option<Vec<IMDBEpisode>>,
    ) -> anyhow::Result<Vec<TorrentItem>> {
        let ordering: Vec<Box<dyn TorrentSearch>> = vec![
            crate::api::yts::YTS::new(),           // Movie
            crate::api::eztv::EZTV::new(),         // TV
            crate::api::therarbg::TheRARBG::new(), // Any
        ];

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
