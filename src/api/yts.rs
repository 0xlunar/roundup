use anyhow::format_err;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, ClientBuilder, Proxy};
use serde::Deserialize;

use crate::api::imdb::{IMDBEpisode, ItemType};
use crate::api::torrent::{MediaQuality, TorrentItem, TorrentSearch};

pub struct YTS {
    client: Client,
    trackers: Vec<String>,
}

impl YTS {
    pub fn new(trackers: &[String], proxy: Option<&String>) -> Box<Self> {
        let mut headers = HeaderMap::new();
        let user_agent = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
        headers.insert("User-Agent", HeaderValue::from_static(user_agent));
        headers.insert("Accept", HeaderValue::from_static("application/json"));

        let client = ClientBuilder::new()
            .default_headers(headers);
        
        let client = match proxy {
            Some(proxy) => client.proxy(Proxy::all(proxy).unwrap()),
            None => client,
        }.build()
            .unwrap();

        Box::new(Self {
            client,
            trackers: trackers.to_vec(),
        })
    }
}

#[async_trait]
impl TorrentSearch for YTS {
    async fn search(
        &self,
        search_term: String,
        imdb_id: Option<String>,
        missing_episodes: Option<Vec<IMDBEpisode>>,
    ) -> anyhow::Result<Vec<TorrentItem>> {
        if missing_episodes.is_some() {
            return Err(format_err!("Not a movie"));
        }
        let q_t = match imdb_id {
            Some(t) => {
                if t.starts_with("tt") {
                    t
                } else {
                    format!("tt{}", t)
                }
            }
            None => search_term,
        };
        let query = [("query_term", q_t.as_str())];

        let resp = self
            .client
            .get("https://yts.mx/api/v2/list_movies.json")
            .query(&query)
            .send()
            .await?;

        let status = resp.status();
        if status.is_server_error() || status.is_client_error() {
            return Err(format_err!("Failed to send request: {}", status));
        }

        let data: YTSListMovieResponse = match resp.text().await {
            Ok(t) => serde_json::from_str(&t)?,
            Err(e) => return Err(e.into()),
        };

        if data.status.ne("ok") {
            return Err(format_err!("Invalid Response, Status: {}", data.status));
        }

        if data.data.movie_count == 0 {
            return Err(format_err!("No Movies available"));
        }

        let mut results = vec![];
        for movie in data.data.movies {
            for torrent in movie.torrents {
                let encoded_title = urlencoding::encode(&movie.title);
                let trackers = self.trackers.join("&tr=");
                let magnet = format!(
                    "magnet:?xt=urn:btih:{}&dn={}&tr={}",
                    torrent.hash, encoded_title, trackers
                );

                let quality = match torrent.quality.to_lowercase().as_str() {
                    "480p" => MediaQuality::_480p,
                    "720p" => MediaQuality::_720p,
                    "1080p" => MediaQuality::_1080p,
                    "1080p.x265" => MediaQuality::BetterThan1080p,
                    "2160p" => MediaQuality::_2160p,
                    _ => MediaQuality::Unknown,
                };

                let item = TorrentItem::new(
                    movie.imdb_code.to_string(),
                    movie.title.to_owned(),
                    magnet,
                    quality,
                    ItemType::Movie,
                    None,
                    None,
                    None,
                    "YTS".to_string(),
                );
                results.push(item);
            }
        }

        Ok(results)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct YTSListMovieResponse {
    status: String,
    data: Data,
}

#[derive(Debug, Clone, Deserialize)]
struct Data {
    movie_count: i64,
    #[serde(default)]
    movies: Vec<Movie>,
}

#[derive(Debug, Clone, Deserialize)]
struct Movie {
    imdb_code: String,
    title: String,
    torrents: Vec<Torrent>,
}

#[derive(Debug, Clone, Deserialize)]
struct Torrent {
    hash: String,
    quality: String,
}
