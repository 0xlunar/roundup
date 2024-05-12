use std::ops::Not;
use std::str::FromStr;
use anyhow::format_err;
use chrono::Local;
use log::error;
use reqwest::{Client, ClientBuilder, StatusCode};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::api::imdb::{ItemType, SearchType};

pub struct MovieDB {
    client: Client,
    api_key: String,
}

#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct MovieDBItem {
    pub id: i32,
    pub imdb_id: String,
    pub title: String,
    pub plot: String,
    pub release_date: chrono::NaiveDate,
    pub image_url: Option<String>,
    pub video_id: Option<String>,
    pub certification: Option<String>,
    pub runtime: Option<i64>,
    pub popularity_rank: Option<i64>,
    pub _type: ItemType,
    pub watchlist: bool,
    #[serde(skip_serializing)]
    pub created_at: chrono::DateTime<Local>,
    #[serde(skip_serializing)]
    pub updated_at: chrono::DateTime<Local>,
}

#[derive(Debug)]
pub struct MovieDBEpisode {
    pub season: i32,
    pub episode: i32,
}

impl MovieDB {
    pub fn new(api_key: &str) -> MovieDB {
        let mut headers = HeaderMap::new();
        headers.insert("Accept", HeaderValue::from_static("application/json"));

        let client = ClientBuilder::new().default_headers(headers).user_agent("roundup/1.0").build().unwrap();

        MovieDB {
            client,
            api_key: api_key.to_string(),
        }
    }

    pub async fn search(&self, query: SearchType) -> anyhow::Result<Vec<MovieDBItem>> {
        match query {
            SearchType::MoviePopular => self.fetch_popular_movies().await,
            SearchType::MovieLatestRelease => self.fetch_latest_movies().await,
            SearchType::TVPopular => self.fetch_popular_tv().await,
            SearchType::TVLatestRelease => self.fetch_latest_tv().await,
            SearchType::Watchlist => unreachable!(),
            SearchType::Downloads => unreachable!(),
            SearchType::Query(q) => self.fetch_query(&q).await
        }
    }

    pub async fn search_tv_episodes(api_key: &str, id: i32, season: i32) -> anyhow::Result<Vec<MovieDBEpisode>> {
        let mut season = season;
        if season.le(&0) {
            season = 1;
        }

        let mut headers = HeaderMap::new();
        headers.insert("Accept", HeaderValue::from_static("application/json"));

        let client = ClientBuilder::new().default_headers(headers).user_agent("roundup/1.0").build().unwrap();

        let query = vec![
            ("language", "en-US"),
            ("api_key", api_key)
        ];

        let resp = client.get(format!("https://api.themoviedb.org/3/tv/{}/season/{}", id, season)).query(&query).send().await?;
        if resp.status().is_client_error() || resp.status().is_server_error() {
            return match resp.status() {
                StatusCode::NOT_FOUND => Ok(Vec::new()),
                _ => {
                    let status = resp.status();
                    let text = resp.text().await?;
                    Err(format_err!("Failed to send request, Status: {}, Text: {}", status, text))
                },
            };
        }

        let text = resp.text().await?;
        let data: TVSeasonDetails = serde_json::from_str(&text)?;

        let mut episodes: Vec<MovieDBEpisode> = data.episodes.iter().map(|x| MovieDBEpisode {
            season: x.season_number,
            episode: x.episode_number,
        }).collect();

        if season.eq(&1) {
            let mut has_finished = false;
            let mut season = season;
            while !has_finished {
                season += 1;
                let mut result = Box::pin(MovieDB::search_tv_episodes(&api_key, id, season)).await?;
                if result.is_empty() {
                    has_finished = true;
                    continue;
                }
                episodes.append(&mut result);
            }
        }

        Ok(episodes)
    }

    async fn fetch_popular_movies(&self) -> anyhow::Result<Vec<MovieDBItem>> {
        let query = vec![("language","en-us"), ("page","1"), ("api_key", &self.api_key)];
        let resp = self.client.get("https://api.themoviedb.org/3/movie/popular").query(&query).send().await?;
        if resp.status().is_client_error() || resp.status().is_server_error() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(format_err!("Failed to send request, Status: {}, Text: {}", status, text))
        }

        let text = resp.text().await?;

        let data: SearchMultiResultResponse = serde_json::from_str(&text)?;

        let movies: Vec<SearchMultiResultMovie> = data.results.into_iter().filter(|x| matches!(x, ResultType::Movie(_))).enumerate().map(|(i, x)| match x {
            ResultType::Movie(mut a) => {
                a.popularity = i as f64;
                a
            },
            _ => unreachable!()
        }).collect();

        let mut tasks = Vec::new();
        for movie in movies {
            tasks.push(MovieDB::fetch_movie_details(&self.api_key, movie));
        }

        let mut results = Vec::new();

        let outcome = futures::prelude::future::join_all(tasks).await;
        for task in outcome {
            match task {
                Ok(t) => results.push(t),
                Err(e) => error!("{}", e),
            };
        }

        Ok(results)
    }
    async fn fetch_latest_movies(&self) -> anyhow::Result<Vec<MovieDBItem>> {
        let now = chrono::offset::Local::now();
        let today = now.format("%Y-%m-%d").to_string();

        let query = vec![
            ("language","en-us"),
            ("page","1"),
            ("api_key", &self.api_key),
            ("include_adult","false"),
            ("include_video","false"),
            ("primary_release_date.lte", &today),
            ("sort_by","primary_release_date.desc"),
            ("vote_count.gte","1"),
            ("with_original_language","en"),
        ];
        let resp = self.client.get("https://api.themoviedb.org/3/discover/movie").query(&query).send().await?;
        if resp.status().is_client_error() || resp.status().is_server_error() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(format_err!("Failed to send request, Status: {}, Text: {}", status, text))
        }

        let text = resp.text().await?;
        let data: SearchMultiResultResponse = serde_json::from_str(&text)?;

        let movies: Vec<SearchMultiResultMovie> = data.results.into_iter().filter_map(|x| match x { ResultType::Movie(a) => Some(a), _ => None }).collect();

        let mut tasks = Vec::new();
        for movie in movies {
            tasks.push(MovieDB::fetch_movie_details(&self.api_key, movie));
        }

        let mut results = Vec::new();

        let outcome = futures::prelude::future::join_all(tasks).await;
        for task in outcome {
            match task {
                Ok(t) => results.push(t),
                Err(e) => error!("{}", e),
            };
        }

        Ok(results)
    }
    async fn fetch_popular_tv(&self) -> anyhow::Result<Vec<MovieDBItem>> {
        let query = vec![
            ("language","en-us"),
            ("page","1"),
            ("include_null_first_air_dates","false"),
            ("include_adult","false"),
            ("sort_by","popularity.desc"),
            ("with_original_language","en"),
            ("without_genres","10767,10763"), // Excluding Talk Shows & News
            ("api_key", &self.api_key),
        ];

        let resp = self.client.get("https://api.themoviedb.org/3/discover/tv").query(&query).send().await?;
        if resp.status().is_client_error() || resp.status().is_server_error() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(format_err!("Failed to send request, Status: {}, Text: {}", status, text))
        }

        let text = resp.text().await?;
        let data: SearchMultiResultResponse = serde_json::from_str(&text)?;

        let tv_shows: Vec<SearchMultiResultTVShow> = data.results.into_iter().filter(|x| matches!(x, ResultType::TVShow(_))).enumerate().map(|(i, x)| match x {
            ResultType::TVShow(mut a) => {
                a.popularity = i as f64;
                a
            },
            _ => unreachable!()
        }).collect();

        let mut tasks = Vec::new();
        for show in tv_shows {
            tasks.push(MovieDB::fetch_tv_details(&self.api_key, show));
        }

        let mut results = Vec::new();

        let outcome = futures::prelude::future::join_all(tasks).await;
        for task in outcome {
            match task {
                Ok(t) => results.push(t),
                Err(e) => error!("{}", e),
            };
        }

        Ok(results)
    }
    async fn fetch_latest_tv(&self) -> anyhow::Result<Vec<MovieDBItem>> {
        let now = chrono::offset::Local::now();
        let today = now.format("%Y-%m-%d").to_string();

        let query = vec![
            ("language","en-us"),
            ("page","1"),
            ("include_null_first_air_dates","false"),
            ("include_adult","false"),
            ("sort_by","first_air_date.desc"),
            ("vote_count.gte","1"),
            ("first_air_date.lte", &today),
            ("with_original_language", "en"),
            ("with_genres","10759|16|35|80|99|18|10751|10762|9648|10764|10765|10766|10768|37"), // All genres except those below, this excludes shows without genres set
            ("without_genres","10767,10763"), // Excluding Talk Shows & News
            ("api_key", &self.api_key),
        ];

        let resp = self.client.get("https://api.themoviedb.org/3/discover/tv").query(&query).send().await?;
        if resp.status().is_client_error() || resp.status().is_server_error() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(format_err!("Failed to send request, Status: {}, Text: {}", status, text))
        }

        let text = resp.text().await?;
        let data: SearchMultiResultResponse = serde_json::from_str(&text)?;

        let tv_shows: Vec<SearchMultiResultTVShow> = data.results.into_iter().filter_map(|x| match x { ResultType::TVShow(a) => Some(a), _ => None }).collect();

        let mut tasks = Vec::new();
        for show in tv_shows {
            tasks.push(MovieDB::fetch_tv_details(&self.api_key, show));
        }

        let mut results = Vec::new();

        let outcome = futures::prelude::future::join_all(tasks).await;
        for task in outcome {
            match task {
                Ok(t) => results.push(t),
                Err(e) => error!("{}", e),
            };
        }

        Ok(results)
    }
    async fn fetch_query(&self, query: &str) -> anyhow::Result<Vec<MovieDBItem>> {
        let query = vec![("query", query), ("api_key", &self.api_key)];

        let resp = self.client.get("https://api.themoviedb.org/3/search/multi").query(&query).send().await?;
        if resp.status().is_client_error() || resp.status().is_server_error() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(format_err!("Failed to send request, Status: {}, Text: {}", status, text))
        }

        let text = resp.text().await?;
        let mut data: SearchMultiResultResponse = serde_json::from_str(&text)?;
        data.results.retain(|t| match t {
            ResultType::Person(_) => false, // Don't care about person results, so remove em
            _ => true,
        });

        let mut movie_tasks = Vec::new();
        let mut tv_tasks = Vec::new();
        for result in data.results {
            match result {
                ResultType::Movie(m) => {
                    if m.release_date.is_empty() {
                        continue;
                    }
                    movie_tasks.push(MovieDB::fetch_movie_details(&self.api_key, m));
                },
                ResultType::TVShow(t) => {
                    if t.first_air_date.is_empty() {
                        continue;
                    }
                    tv_tasks.push(MovieDB::fetch_tv_details(&self.api_key, t));
                },
                _ => unreachable!(),
            };
        }

        let mut results = Vec::new();
        let mut output =  futures::prelude::future::join_all(movie_tasks).await;
        let mut output_tv =  futures::prelude::future::join_all(tv_tasks).await;
        output.append(&mut output_tv);
        for task in output {
            match task {
                Ok(t) => results.push(t),
                Err(e) => error!("{}", e),
            };
        }

        Ok(results)
    }

    async fn fetch_movie_details(api_key: &str, initial_search: SearchMultiResultMovie) -> anyhow::Result<MovieDBItem> {
        let mut headers = HeaderMap::new();
        headers.insert("Accept", HeaderValue::from_static("application/json"));

        let client = ClientBuilder::new().default_headers(headers).user_agent("roundup/1.0").build().unwrap();

        let query = vec![
                ("language", "en-US"),
                ("append_to_response", "videos,release_dates"),
                ("api_key", api_key)
        ];

        let resp = client.get(format!("https://api.themoviedb.org/3/movie/{}", initial_search.id)).query(&query).send().await?;
        if resp.status().is_client_error() || resp.status().is_server_error() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(format_err!("Failed to send request, Status: {}, Text: {}", status, text))
        }

        let text = resp.text().await?;
        let data: MovieDetailsResponse = serde_json::from_str(&text)?;

        if data.id != initial_search.id {
            return Err(format_err!("Error fetching external ID, got different id"));
        }

        let video_id = match data.videos.results.iter().find(|x| x._type.eq("Trailer")) {
            Some(t) => {
                if t.site.eq("YouTube") {
                    Some(t.key.to_string())
                } else {
                    None
                }
            },
            None => match data.videos.results.first() {
                Some(t) => {
                    if t.site.eq("YouTube") {
                        Some(t.key.to_string())
                    } else {
                        None
                    }
                },
                None => None,
            }
        };

        let certification = match data.release_dates.results.iter().find(|x| x.iso_3166_1.eq("AU") || x.iso_3166_1.eq("US")) {
            Some(t) => t.release_dates.iter().find(|x| x.certification.is_empty().not()).map(|t| t.certification.to_string()),
            None => None,
        };

        let mut popularity_rank = None;
        if initial_search.popularity.fract() == 0.0 && initial_search.popularity.le(&100.00){
            popularity_rank = Some(initial_search.popularity as i64);
        }

        let movie_item = MovieDBItem {
            id: data.id,
            imdb_id: data.imdb_id,
            title: data.title,
            image_url: data.poster_path,
            plot: data.overview,
            release_date: chrono::NaiveDate::from_str(&data.release_date).unwrap(),
            video_id,
            certification,
            runtime: Some(data.runtime),
            popularity_rank,
            _type: ItemType::Movie,
            watchlist: false,
            created_at: Local::now(),
            updated_at: Local::now(),
        };

        Ok(movie_item)
    }
    async fn fetch_tv_details(api_key: &str, initial_search: SearchMultiResultTVShow) -> anyhow::Result<MovieDBItem> {
        let mut headers = HeaderMap::new();
        headers.insert("Accept", HeaderValue::from_static("application/json"));

        let client = ClientBuilder::new().default_headers(headers).user_agent("roundup/1.0").build().unwrap();

        let query = vec![
            ("language", "en-US"),
            ("append_to_response", "videos,content_ratings,external_ids"),
            ("api_key", api_key)
        ];

        let resp = client.get(format!("https://api.themoviedb.org/3/tv/{}", initial_search.id)).query(&query).send().await?;
        if resp.status().is_client_error() || resp.status().is_server_error() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(format_err!("Failed to send request, Status: {}, Text: {}", status, text))
        }

        let text = resp.text().await?;
        let data: TVDetailsResponse = serde_json::from_str(&text)?;

        if data.id != initial_search.id {
            return Err(format_err!("Error fetching external ID, got different id"));
        }

        let video_id = match data.videos.results.iter().find(|x| x._type.eq("Trailer")) {
            Some(t) => {
                if t.site.eq("YouTube") {
                    Some(t.key.to_string())
                } else {
                    None
                }
            },
            None => match data.videos.results.first() {
                Some(t) => {
                    if t.site.eq("YouTube") {
                        Some(t.key.to_string())
                    } else {
                        None
                    }
                },
                None => None,
            }
        };

        let certification = data.content_ratings.results.iter().find(|x| x.iso_3166_1.eq("AU") || x.iso_3166_1.eq("US")).map(|t| t.rating.to_string());

        let mut popularity_rank = None;
        if initial_search.popularity.fract() == 0.0 && initial_search.popularity.le(&100.00){
            popularity_rank = Some(initial_search.popularity as i64);
        }

        let imdb_id = match data.external_ids.imdb_id {
            Some(t) => t,
            None => return Err(format_err!("Missing IMDB ID")),
        };

        let tv_item = MovieDBItem {
            id: data.id,
            imdb_id,
            title: data.name,
            image_url: initial_search.poster_path.to_owned(),
            plot: data.overview,
            release_date: chrono::NaiveDate::from_str(&data.first_air_date).unwrap(),
            video_id,
            certification,
            runtime: None,
            popularity_rank,
            _type: ItemType::TvShow,
            watchlist: false,
            created_at: Local::now(),
            updated_at: Local::now(),
        };

        Ok(tv_item)
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ResultType {
    Movie(SearchMultiResultMovie),
    TVShow(SearchMultiResultTVShow),
    Person(Value),
}
#[derive(Debug, Deserialize)]
struct SearchMultiResultResponse {
    page: u32,
    results: Vec<ResultType>,
}
#[derive(Debug, Deserialize, Clone)]
struct SearchMultiResultMovie {
    id: i32,
    title: String,
    overview: String,
    poster_path: Option<String>,
    popularity: f64,
    release_date: String,
}
#[derive(Debug, Deserialize, Clone)]
struct SearchMultiResultTVShow {
    id: i32,
    name: String,
    overview: String,
    poster_path: Option<String>,
    popularity: f64,
    first_air_date: String,
}

#[derive(Debug, Deserialize)]
struct ExternalIdResponse {
    id: i32,
    imdb_id: String,
}

#[derive(Debug, Deserialize)]
struct MovieDetailsResponse {
    id: i32,
    imdb_id: String,
    overview: String,
    title: String,
    release_date: String,
    popularity: f64,
    runtime: i64,
    poster_path: Option<String>,
    release_dates: MovieReleaseDates,
    videos: VideosResponse,
}
#[derive(Debug, Deserialize)]
struct MovieReleaseDates {
    results: Vec<MovieReleaseDatesItem>,
}
#[derive(Debug, Deserialize)]
struct MovieReleaseDatesItem {
    iso_3166_1: String,
    release_dates: Vec<MovieReleaseDatesItemInner>,
}
#[derive(Debug, Deserialize)]
struct MovieReleaseDatesItemInner {
    certification: String,
}
#[derive(Debug, Deserialize)]
struct VideosResponse {
    results: Vec<VideoItem>,
}
#[derive(Debug, Deserialize)]
struct VideoItem {
    name: String,
    key: String,
    site: String,
    official: bool,
    #[serde(rename = "type")]
    _type: String,
}

#[derive(Debug, Deserialize)]
struct TVDetailsResponse {
    id: i32,
    name: String,
    first_air_date: String,
    number_of_episodes: i32,
    number_of_seasons: i32,
    overview: String,
    videos: VideosResponse,
    content_ratings: TVContentRatingsResponse,
    external_ids: TVDetailsExternalIds
}
#[derive(Debug, Deserialize)]
struct TVContentRatingsResponse {
    results: Vec<ContentRatingsResultItem>,
}
#[derive(Debug, Deserialize)]
struct ContentRatingsResultItem {
    iso_3166_1: String,
    rating: String,
}
#[derive(Debug, Deserialize)]
struct TVDetailsExternalIds {
    imdb_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TVSeasonDetails {
    episodes: Vec<TVSeasonDetailsEpisode>,
}

#[derive(Debug, Deserialize)]
struct TVSeasonDetailsEpisode {
    episode_number: i32,
    season_number: i32,
}