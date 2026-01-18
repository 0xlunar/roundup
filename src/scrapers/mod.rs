pub mod imdb;
mod yts;

use crate::AppConfig;
use crate::database::Database;
use crate::torrent::TorrentIdentifier;
use anyhow::format_err;
use serde::de::{Error, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;
use wreq::Client;

#[async_trait::async_trait]
pub trait TorrentScraper: Send + Sync {
    async fn search(&self, query: TorrentSearch) -> Result<Vec<Torrent>, TorrentScraperError>;

    fn source(&self) -> &'static str;
}

pub struct TorrentSearcher {
    config: Arc<Mutex<AppConfig>>,
    client: Client,
    database: Arc<Database>,
    scrapers: Vec<Box<dyn TorrentScraper>>,
}

impl TorrentSearcher {
    pub async fn new(
        config: Arc<Mutex<AppConfig>>,
        client: Client,
        database: Arc<Database>,
    ) -> Self {
        let yts_base_url = {
            let lock = config.lock().await;
            lock.yts_base_url.clone()
        };

        let scrapers: Vec<Box<dyn TorrentScraper>> = vec![yts::YTS::new(
            yts_base_url,
            client.clone(),
            database.clone(),
        )];

        Self {
            config,
            client,
            database,
            scrapers,
        }
    }
    pub async fn search(&self, query: TorrentSearch) -> Result<Vec<Torrent>, TorrentScraperError> {
        let scrapers = self
            .scrapers
            .iter()
            .map(|scraper| scraper.search(query.clone()));

        let torrents = futures::future::select_ok(scrapers).await;
        match torrents {
            Ok((torrents, _)) => Ok(torrents),
            Err(err) => Err(err),
        }
    }
}

pub struct Torrent {
    pub torrent: TorrentIdentifier,
    pub source: String,
    pub title: String,
    pub media_type: TorrentMediaType,
}

pub enum TorrentMediaType {
    Movie,
    TvShowEpisode {
        season: i64,
        episode: i64,
    },
    TvShowSeason {
        season: i64,
    },
    TvShowSeasonPack {
        // ie Seasons 2-5 Pack
        season_first: i64, // First season available in the pack
        season_last: i64,  // Last season available in the pack
    },
}

#[derive(Debug, Clone)]
pub enum TorrentSearch {
    Query(Arc<str>),
    IMDb(IMDbId),
}

#[derive(Debug)]
pub enum MediaQuality {
    _240p,
    _360p,
    _480p,
    _720p,
    _1080p,
    _1440p,
    _2160p,  // 4K
    _4320p,  // 8K
    _8640p,  // 16K
    _17280p, // 32K
}

pub enum TorrentScraperError {
    TorrentMismatch,
    Anyhow(anyhow::Error),
}

#[derive(Debug, Clone, PartialEq, sqlx::Type)]
#[sqlx(transparent, no_pg_array)]
pub struct IMDbId(Arc<str>);
impl<'a> IMDbId {
    pub fn new(id: &'a str) -> Result<Self, String> {
        if id.len() >= 9
            && id.as_bytes()[0] == b't'
            && id.as_bytes()[1] == b't'
            && id[2..].chars().all(|c| c.is_ascii_digit())
        {
            Ok(Self(id.into()))
        } else {
            Err(format!("{id} is not an IMDb Id"))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl Display for IMDbId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// impl From<String> for IMDbId {
//     fn from(value: String) -> Self {
//         IMDbId::new(&value).expect("Must be valid IMDB Id")
//     }
// }
//
// impl From<&str> for IMDbId {
//     fn from(value: &str) -> Self {
//         IMDbId::new(value).expect("Must be valid IMDB Id")
//     }
// }

#[derive(Debug, Clone, sqlx::Type)]
#[sqlx(type_name = "media_type", rename_all = "lowercase")]
pub enum IMDbMediaType {
    Movie,
    TvShow,
}

impl IMDbMediaType {
    pub fn to_db_enum(&self) -> &'static str {
        match self {
            IMDbMediaType::Movie => "movie",
            IMDbMediaType::TvShow => "tvshow",
        }
    }
    pub fn as_payload_str(&self) -> &'static str {
        match self {
            IMDbMediaType::Movie => "1",
            IMDbMediaType::TvShow => "2",
        }
    }

    pub fn as_u32(&self) -> u32 {
        match self {
            IMDbMediaType::Movie => 1,
            IMDbMediaType::TvShow => 2,
        }
    }
}

impl Display for IMDbMediaType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_db_enum())
    }
}

impl Display for MediaQuality {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaQuality::_240p => f.write_str("240p"),
            MediaQuality::_360p => f.write_str("360p"),
            MediaQuality::_480p => f.write_str("480p"),
            MediaQuality::_720p => f.write_str("720p"),
            MediaQuality::_1080p => f.write_str("1080p"),
            MediaQuality::_1440p => f.write_str("1440p"),
            MediaQuality::_2160p => f.write_str("2160p"),
            MediaQuality::_4320p => f.write_str("4320p"),
            MediaQuality::_8640p => f.write_str("8640p"),
            MediaQuality::_17280p => f.write_str("17280p"),
        }
    }
}

impl FromStr for MediaQuality {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "240p" => Ok(MediaQuality::_240p),
            "360p" => Ok(MediaQuality::_360p),
            "480p" => Ok(MediaQuality::_480p),
            "720p" => Ok(MediaQuality::_720p),
            "1080p" => Ok(MediaQuality::_1080p),
            "1440p" => Ok(MediaQuality::_1440p),
            "2160p" | "4K" | "4k" => Ok(MediaQuality::_2160p),
            "4320p" | "8K" | "8k" => Ok(MediaQuality::_4320p),
            "8640p" | "16K" | "16k" => Ok(MediaQuality::_8640p),
            "17280p" | "32K" | "32k" => Ok(MediaQuality::_17280p),
            invalid => Err(format_err!("received invalid quality: {invalid}")),
        }
    }
}

impl<'de> Deserialize<'de> for MediaQuality {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(MediaQualityVisitor)
    }
}

struct MediaQualityVisitor;
impl<'de> Visitor<'de> for MediaQualityVisitor {
    type Value = MediaQuality;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("a valid quality type of at least 240p and max 17280p")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        MediaQuality::from_str(v).map_err(serde::de::Error::custom)
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: Error,
    {
        MediaQuality::from_str(&v).map_err(serde::de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for IMDbId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(IMDbIdVisitor)
    }
}

struct IMDbIdVisitor;
impl<'de> Visitor<'de> for IMDbIdVisitor {
    type Value = IMDbId;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("a value beginning with tt and having at least 7 digits proceeding")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        Self::Value::new(v).map_err(serde::de::Error::custom)
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: Error,
    {
        Self::Value::new(&v).map_err(serde::de::Error::custom)
    }
}

mod test {
    use crate::scrapers::IMDbId;

    #[test]
    fn valid_ids() {
        assert_eq!(
            IMDbId::new("tt0121955"),
            Ok(IMDbId("tt0121955".into())),
            "Failed to parse valid imdb id"
        );

        assert_eq!(
            IMDbId::new("tt0436992"),
            Ok(IMDbId("tt0436992".into())),
            "Failed to parse valid imdb id"
        );

        assert_eq!(
            IMDbId::new("tt4574334"),
            Ok(IMDbId("tt4574334".into())),
            "Failed to parse valid imdb id"
        );
    }

    #[test]
    fn errors_if_starts_with_tt_but_not_all_ascii_numeric_afterwards() {
        assert_eq!(
            IMDbId::new("tt128l173"),
            Err("tt128l173 is not an IMDb Id".to_string()),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("ttabv12345"),
            Err("ttabv12345 is not an IMDb Id".to_string()),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("tt8333838i"),
            Err("tt8333838i is not an IMDb Id".to_string()),
            "Got valid id for invalid id"
        );
    }

    #[test]
    fn errors_if_starts_with_numbers() {
        assert_eq!(
            IMDbId::new("012195512"),
            Err("012195512 is not an IMDb Id".to_string()),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("012195573"),
            Err("012195573 is not an IMDb Id".to_string()),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("012195551"),
            Err("012195551 is not an IMDb Id".to_string()),
            "Got valid id for invalid id"
        );
    }

    #[test]
    fn errors_if_starts_with_non_tt_character() {
        assert_eq!(
            IMDbId::new("aa2195512"),
            Err("aa2195512 is not an IMDb Id".to_string()),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("cx2195573"),
            Err("cx2195573 is not an IMDb Id".to_string()),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("du2195551"),
            Err("du2195551 is not an IMDb Id".to_string()),
            "Got valid id for invalid id"
        );
    }

    #[test]
    fn errors_if_less_than_9_characters() {
        assert_eq!(
            IMDbId::new("tt01219"),
            Err("tt01219 is not an IMDb Id".to_string()),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("b1012195"),
            Err("b1012195 is not an IMDb Id".to_string()),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("aa012391"),
            Err("aa012391 is not an IMDb Id".to_string()),
            "Got valid id for invalid id"
        );
    }
}
