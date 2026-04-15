use crate::database::imdb::IMDbDB;
use crate::database::watchlist::WatchlistDB;
use crate::database::{Database, TorrentDB};
use crate::scrapers::imdb::{IMDbItem, IMDbSortBy};
use crate::scrapers::IMDbMediaType;
use crate::server::components::{download_item_cards, item_cards};
use crate::{scrapers, Expiration};
use actix_web::http::StatusCode;
use actix_web::web::{Data, Query};
use actix_web::{post, Error, HttpResponse};
use maud::{html, Markup};
use moka::future::Cache;
use serde::de::Visitor;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize)]
struct SearchQuery {
    query: QueryType,
}

type SearchCache = Cache<QueryType, (Expiration, Arc<Vec<IMDbItem>>)>;

macro_rules! internal_server_error {
    ($msg:expr) => {{
        Ok(HttpResponse::with_body(
            StatusCode::INTERNAL_SERVER_ERROR,
            html! {
                ($msg)
            },
        ))
    }};
}

#[post("/search")]
pub async fn search(
    client: Data<wreq::Client>,
    db: Data<Database>,
    search_cache: Data<SearchCache>,
    search_query: Query<SearchQuery>,
) -> Result<HttpResponse<Markup>, Error> {
    if let Some((_, items)) = search_cache.get(&search_query.query).await {
        return Ok(HttpResponse::with_body(StatusCode::OK, item_cards(&items)));
    };

    let scraper = scrapers::imdb::IMDbScraper::new(client, db.clone());

    match &search_query.query {
        QueryType::MovieTop => {
            let top_movies = scraper
                .top_media(IMDbMediaType::Movie, IMDbSortBy::Popularity)
                .await;
            match top_movies {
                Ok(movies) => {
                    let movies = movies
                        .iter()
                        .map(|movie| movie.item.item)
                        .collect::<Vec<_>>();
                    let markup = item_cards(&movies);

                    let arc_movies = Arc::new(movies);
                    search_cache
                        .insert(QueryType::MovieTop, (Expiration::Long, arc_movies))
                        .await;

                    Ok(HttpResponse::with_body(StatusCode::OK, markup))
                }
                Err(err) => internal_server_error!(err.to_string()),
            }
        }
        QueryType::MovieCalender => {
            let calendar = scraper.release_calendar(IMDbMediaType::Movie).await;
            match calendar {
                Ok(movies) => {
                    let movies = movies.iter().map(|movie| movie.item).collect::<Vec<_>>();
                    let markup = item_cards(&movies);

                    let arc_movies = Arc::new(movies);
                    search_cache
                        .insert(QueryType::MovieCalender, (Expiration::Long, arc_movies))
                        .await;

                    Ok(HttpResponse::with_body(StatusCode::OK, markup))
                }
                Err(err) => internal_server_error!(err.to_string()),
            }
        }
        QueryType::TvTop => {
            let top_tv = scraper
                .top_media(IMDbMediaType::TvShow, IMDbSortBy::Popularity)
                .await;
            match top_tv {
                Ok(tv) => {
                    let tv = tv.iter().map(|tv| tv.item.item).collect::<Vec<_>>();
                    let markup = item_cards(&tv);

                    let arc_tvshows = Arc::new(tv);
                    search_cache
                        .insert(QueryType::MovieTop, (Expiration::Long, arc_tvshows))
                        .await;

                    Ok(HttpResponse::with_body(StatusCode::OK, markup))
                }
                Err(err) => internal_server_error!(err.to_string()),
            }
        }
        QueryType::TvCalender => {
            let calendar = scraper.release_calendar(IMDbMediaType::TvShow).await;
            match calendar {
                Ok(tv) => {
                    let tv = tv.iter().map(|tv| tv.item).collect::<Vec<_>>();
                    let markup = item_cards(&tv);

                    let arc_movies = Arc::new(tv);
                    search_cache
                        .insert(QueryType::TvCalender, (Expiration::Long, arc_movies))
                        .await;

                    Ok(HttpResponse::with_body(StatusCode::OK, markup))
                }
                Err(err) => internal_server_error!(err.to_string()),
            }
        }
        QueryType::Watchlist => {
            let imdb_db = IMDbDB::new(&db);
            let watchlist = WatchlistDB::new(&db);
            match watchlist.get_items().await {
                Ok(items) => {
                    let items = match imdb_db.get_items(&items).await {
                        Ok(items) => items,
                        Err(err) => return internal_server_error!(err.to_string()),
                    };

                    let markup = item_cards(&items);
                    Ok(HttpResponse::with_body(StatusCode::OK, markup))
                }
                Err(err) => internal_server_error!(err.to_string()),
            }
        }
        QueryType::Downloads => {
            let imdb_db = IMDbDB::new(&db);
            let torrents = TorrentDB::new(&db);
            match torrents.get_all_with_imdb().await {
                Ok(items) => {
                    let markup = download_item_cards(&items);
                    Ok(HttpResponse::with_body(StatusCode::OK, markup))
                }
                Err(err) => internal_server_error!(err.to_string()),
            }
        }
        QueryType::Query(query) => {
            let media = scraper.search(&query).await;
            match media {
                Ok(media) => {
                    let markup = item_cards(&media);

                    let arc_media = Arc::new(media);
                    search_cache
                        .insert(search_query.query, (Expiration::Short, arc_media))
                        .await;

                    Ok(HttpResponse::with_body(StatusCode::OK, markup))
                }
                Err(err) => internal_server_error!(err.to_string()),
            }
        }
    }
}

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub enum QueryType {
    MovieTop,
    MovieCalender,
    TvTop,
    TvCalender,
    Watchlist,
    Downloads,
    Query(String),
}

impl Display for QueryType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self {
            QueryType::MovieTop => f.write_str("MovieTop"),
            QueryType::MovieCalender => f.write_str("MovieCalendar"),
            QueryType::TvTop => f.write_str("TvTop"),
            QueryType::TvCalender => f.write_str("TvCalendar"),
            QueryType::Watchlist => f.write_str("Watchlist"),
            QueryType::Downloads => f.write_str("Downloads"),
            QueryType::Query(query) => f.write_str(query),
        }
    }
}

impl From<&str> for QueryType {
    fn from(value: &str) -> Self {
        match value {
            "MovieTop" => Self::MovieTop,
            "MovieCalendar" => Self::MovieCalender,
            "TvTop" => Self::TvTop,
            "TvCalendar" => Self::TvCalender,
            "Watchlist" => Self::Watchlist,
            "Downloads" => Self::Downloads,
            query => Self::Query(query.to_string()),
        }
    }
}

impl From<String> for QueryType {
    fn from(value: String) -> Self {
        value.as_str().into()
    }
}

impl Serialize for QueryType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

struct SearchTypeVisitor;
impl<'de> Visitor<'de> for SearchTypeVisitor {
    type Value = QueryType;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("a string or &str")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(v.into())
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(v.into())
    }
}

impl<'de> Deserialize<'de> for QueryType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_string(SearchTypeVisitor)
    }
}
