use std::ops::Not;

use anyhow::format_err;
use async_trait::async_trait;
use rayon::prelude::*;
use rquest::header::{HeaderMap, HeaderValue};
use rquest::Client;
use serde::Deserialize;

use crate::api::imdb::{IMDBEpisode, ItemType};
use crate::api::torrent::{MediaQuality, TorrentItem, TorrentSearch};

pub struct EZTV {
    client: Client,
}

impl EZTV {
    pub fn new(proxy: Option<&String>) -> Box<Self> {
        let mut headers = HeaderMap::new();
        headers.insert("Accept", HeaderValue::from_static("application/json"));

        let user_agent = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
        let mut client = rquest::ClientBuilder::new()
            .user_agent(user_agent)
            .default_headers(headers)
            .zstd(true)
            .brotli(true)
            .deflate(true)
            .gzip(true);

        client = match proxy {
            Some(p) => client.proxy(p.to_owned()),
            None => client,
        };

        let client = client.build().unwrap();

        Box::new(Self { client })
    }

    async fn get_torrents(
        &self,
        query: &str,
        page: u32,
    ) -> anyhow::Result<EZTVTorrentListResponse> {
        let mut query = vec![("imdb_id", query), ("limit", "100")];
        let p_s = page.to_string();
        if page.gt(&1) {
            query.push(("page", p_s.as_str()))
        }

        let resp = self
            .client
            .get("https://eztvx.to/api/get-torrents")
            .query(&query)
            .send()
            .await?;

        let status = resp.status();
        if status.is_client_error() || status.is_server_error() {
            return Err(format_err!("Failed to make request: {}", status));
        }

        let data = match resp.text().await {
            Ok(t) => serde_json::from_str(&t)?,
            Err(e) => return Err(e.into()),
        };

        Ok(data)
    }
}

#[async_trait]
impl TorrentSearch for EZTV {
    async fn search(
        &self,
        _: String,
        imdb_id: Option<String>,
        tv_episodes: Option<Vec<IMDBEpisode>>,
    ) -> anyhow::Result<Vec<TorrentItem>> {
        if tv_episodes.is_none() {
            return Err(format_err!("Not a TV show"));
        }

        let imdb_id_clone = imdb_id.clone().unwrap();
        let query = match imdb_id {
            Some(i) => i,
            None => return Err(format_err!("Missing IMDB_ID")),
        };
        let query = match query.strip_prefix("tt") {
            Some(t) => t,
            None => query.as_str(),
        };
        let mut data = self.get_torrents(query, 1).await?;

        if data.torrents_count.gt(&100) {
            let total_pages = (data.torrents_count as f64 / 100.0).ceil() as u32;
            for page in 2..=total_pages {
                let mut d = self.get_torrents(query, page).await?;
                data.torrents.append(&mut d.torrents);
            }
        }

        data.torrents = data
            .torrents
            .into_par_iter()
            .filter(|a| a.seeds.gt(&0))
            .collect::<Vec<EZTVTorrent>>();
        data.torrents.sort_by(|a, b| b.seeds.cmp(&a.seeds));

        let episodes = tv_episodes.unwrap();

        let mut torrents: Vec<TorrentItem> = data
            .torrents
            .par_iter()
            .filter(|t| {
                let season = t.season.parse::<i32>().unwrap();
                let episode = t.episode.parse::<i32>().unwrap();
                episodes
                    .iter()
                    .any(|e| e.season == season && e.episode == episode)
                    && t.filename.contains(".multi").not() // Remove Multilingual Torrents
            })
            .map(|t| {
                let quality = t
                    .title
                    .split(' ')
                    .find(|q| q.ends_with("0p"))
                    .unwrap_or("unknown");

                let quality = match quality {
                    "480p" => MediaQuality::_480p,
                    "720p" => MediaQuality::_720p,
                    "1080p" => MediaQuality::_1080p,
                    "2160p" => MediaQuality::_2160p,
                    _ => MediaQuality::Unknown,
                };
                let season = t.season.parse::<i32>().unwrap();
                let mut episode = t.episode.parse::<i32>().unwrap();

                let lower_case_title = t.title.to_lowercase();
                if episode == 0
                    && (lower_case_title.contains("complete")
                        || (!lower_case_title.contains("e0")
                            && !lower_case_title.contains("episode")))
                {
                    episode = -1;
                }

                let q_s = quality.to_string();
                let mut title = t
                    .title
                    .clone()
                    .drain(..t.title.find(&q_s).unwrap_or(t.title.len()))
                    .collect::<String>();
                title.push(' ');
                title.push_str(&q_s);

                TorrentItem::new(
                    imdb_id_clone.clone(),
                    title,
                    t.magnet_url.to_string(),
                    quality,
                    ItemType::TvShow,
                    Some(season),
                    Some(episode),
                    None,
                    "EZTV".to_string(),
                )
            })
            .filter(|t| !matches!(t.quality, MediaQuality::Unknown))
            .collect();

        torrents.sort_by(|a, b| {
            let a_s = a.season.as_ref().unwrap();
            let b_s = b.season.as_ref().unwrap();

            if a_s == b_s {
                let a_e = a.episode.as_ref().unwrap();
                let b_e = b.episode.as_ref().unwrap();
                if a_e == b_e {
                    b.quality.cmp(&a.quality)
                } else {
                    a_e.cmp(b_e)
                }
            } else {
                a_s.cmp(b_s)
            }
        });
        torrents.dedup_by(|a, b| {
            let a_s = a.season.as_ref().unwrap();
            let b_s = b.season.as_ref().unwrap();

            if a_s == b_s {
                b.episode.as_ref().unwrap() == a.episode.as_ref().unwrap() && a.quality == b.quality
            } else {
                false
            }
        });

        Ok(torrents)
    }
}

#[derive(Deserialize, Debug)]
struct EZTVTorrentListResponse {
    torrents_count: i64,
    torrents: Vec<EZTVTorrent>,
}

#[derive(Deserialize, Debug)]
struct EZTVTorrent {
    filename: String,
    magnet_url: String,
    title: String,
    season: String,
    episode: String,
    seeds: i64,
}
