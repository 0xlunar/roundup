use crate::database::Database;
use crate::scrapers::{
    IMDbId, MediaQuality, Torrent, TorrentMediaType, TorrentScraper, TorrentScraperError,
    TorrentSearch,
};
use crate::torrent::TorrentIdentifier;
use actix_web::web::Data;
use anyhow::format_err;
use serde::Deserialize;
use std::fmt::Debug;
use wreq::Client;

#[derive(Clone)]
pub struct YTS {
    base_url: String,
    client: Data<Client>,
    database: Data<Database>,
    trackers: Vec<String>,
}

macro_rules! yts_api_query {
    ($self:expr, $url:expr, $query:expr, $return_type:ty) => {{
        let response = match $self.client.get($url).query($query).send().await {
            Ok(response) => response,
            Err(err) => return Err(TorrentScraperError::Anyhow(err.into())),
        };

        if response.status().is_server_error() || response.status().is_client_error() {
            return Err(TorrentScraperError::Anyhow(format_err!(
                "Failed to get response: {}",
                response.status()
            )));
        }

        let data: YTSResponse<$return_type> = match response.bytes().await {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(data) => data,
                Err(err) => return Err(TorrentScraperError::Anyhow(err.into())),
            },
            Err(err) => return Err(TorrentScraperError::Anyhow(err.into())),
        };

        data
    }};
}

impl YTS {
    pub fn new(base_url: String, client: Data<Client>, database: Data<Database>) -> Box<Self> {
        Box::new(Self {
            base_url,
            client,
            database,
            trackers: vec![],
        })
    }

    async fn list_movies(&self, query: &str) -> Result<Vec<Torrent>, TorrentScraperError> {
        let url = format!("{}/api/v2/movie_details.json", self.base_url);
        let payload = &[("limit", "1"), ("query_term", query)];

        let data = yts_api_query!(&self, url, payload, YTSListMovies);

        let movie = match data.data.movies.into_iter().find(|movie| {
            movie.imdb_code == query || movie.title.to_lowercase() == query.to_lowercase()
        }) {
            Some(movie) => movie,
            None => return Err(TorrentScraperError::TorrentMismatch),
        };

        Ok(self.yts_torrent_to_torrent(movie.torrents, movie.slug))
    }

    async fn get_details(&self, imdb_id: IMDbId) -> Result<Vec<Torrent>, TorrentScraperError> {
        let url = format!("{}/api/v2/movie_details.json", self.base_url);
        let payload = &[("imdb_id", imdb_id.as_str())];

        let data = yts_api_query!(&self, url, payload, YTSMovieDetails);

        let movie = data.data.movie;
        if movie.imdb_code != imdb_id.as_str() {
            return Err(TorrentScraperError::TorrentMismatch);
        }

        Ok(self.yts_torrent_to_torrent(movie.torrents, movie.slug))
    }

    fn yts_torrent_to_torrent(&self, torrents: Vec<YTSTorrent>, name: String) -> Vec<Torrent> {
        torrents
            .into_iter()
            .filter_map(|torrent| {
                let named_torrent = TorrentIntermediate {
                    torrent,
                    source: self.source().to_string(),
                    name: name.clone(),
                };
                named_torrent.into()
            })
            .collect::<Vec<_>>()
    }
}

#[async_trait::async_trait]
impl TorrentScraper for YTS {
    async fn search(&self, query: TorrentSearch) -> Result<Vec<Torrent>, TorrentScraperError> {
        match query {
            TorrentSearch::Query(query) => self.list_movies(&query).await,
            TorrentSearch::IMDb(imdb) => self.get_details(imdb).await,
        }
    }

    fn source(&self) -> &'static str {
        "YTS"
    }
}

impl From<TorrentIntermediate> for Option<Torrent> {
    fn from(value: TorrentIntermediate) -> Self {
        Some(Torrent {
            torrent: TorrentIdentifier::new_hash(&value.torrent.hash),
            source: value.source,
            title: value.name,
            media_type: TorrentMediaType::Movie,
            media_quality: value.torrent.quality,
        })
    }
}

#[derive(Debug, Deserialize)]
struct YTSResponse<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct YTSListMovies {
    movies: Vec<YTSMovie>,
}

#[derive(Debug, Deserialize)]
struct YTSMovieDetails {
    movie: YTSMovie,
}

#[derive(Debug, Deserialize)]
struct YTSMovie {
    imdb_code: String,
    slug: String,  // for magnet creation
    title: String, // for ListMovies searching
    torrents: Vec<YTSTorrent>,
}

#[derive(Debug, Deserialize)]
struct YTSTorrent {
    hash: String,
    quality: MediaQuality,
    size_bytes: u64,
}

struct TorrentIntermediate {
    torrent: YTSTorrent,
    name: String,
    source: String,
}
