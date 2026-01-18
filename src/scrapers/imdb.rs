use super::{IMDbId, IMDbMediaType};
use crate::database::Database;
use crate::database::imdb::IMDbDB;
use anyhow::format_err;
use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use log::warn;
use scraper::{Html, Selector};
use serde::de::{Error, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, Row};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;

static DEFAULT_IMAGE_URL: &str =
    "https://upload.wikimedia.org/wikipedia/commons/1/14/No_Image_Available.jpg";
#[derive(Clone)]
pub struct IMDbScraper {
    client: wreq::Client,
    database: Arc<Database>,
}

#[derive(Debug, Clone)]
pub enum IMDbSortBy {
    Popularity,
    ReleaseOrder,
}

#[derive(FromRow)]
pub struct IMDbItem {
    pub id: IMDbId,
    pub title: String,
    pub year: i64,
    pub image_url: Option<String>,
    pub _type: IMDbMediaType,
}

#[derive(FromRow)]
pub struct IMDbDetailedItem {
    #[sqlx(flatten)]
    pub item: IMDbItem,
    pub plot: String,
    pub runtime_seconds: i64,
    pub video_url: Option<String>,
    pub release_date: chrono::DateTime<Utc>,
    #[sqlx(json(nullable))]
    pub seasons: Option<Vec<IMDbSeason>>,
}

#[derive(Serialize, Deserialize)]
pub struct IMDbSeason {
    pub season: i64,
    pub episodes: Vec<i64>,
}

#[derive(FromRow)]
pub struct IMDbReleaseCalendarItem {
    pub release_date: chrono::DateTime<Utc>,
    #[sqlx(flatten)]
    pub item: IMDbItem,
}

#[derive(FromRow)]
pub struct IMDbTopMediaItem {
    pub ranking: i64,
    #[sqlx(flatten)]
    pub item: IMDbReleaseCalendarItem,
}

#[derive(Debug)]
pub enum IMDbScraperError {
    SearchError(String),
    DetailPageError(String),
    TopMediaError(String),
    ReleaseCalendarError(String),
    Error(anyhow::Error),
}

impl IMDbScraper {
    pub fn new(client: wreq::Client, database: Arc<Database>) -> Self {
        Self { client, database }
    }

    pub async fn search(&self, query: &str) -> Result<Vec<IMDbItem>, IMDbScraperError> {
        let url = format!(
            "https://v3.sg.media-imdb.com/suggestion/x/{}.json?includeVideos=1",
            query
        );
        let response = match self
            .client
            .get(url)
            .header("Accept", "application/json")
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => return Err(IMDbScraperError::Error(err.into())),
        };

        if response.status().is_server_error() || response.status().is_client_error() {
            return Err(IMDbScraperError::SearchError(format!(
                "Failed to get response: {}",
                response.status()
            )));
        }

        let data: IMDbSearchQueryResponse = match response.bytes().await {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(data) => data,
                Err(err) => return Err(IMDbScraperError::Error(err.into())),
            },
            Err(err) => return Err(IMDbScraperError::Error(err.into())),
        };

        let data = data
            .data
            .into_iter()
            .filter_map(|data| data.into())
            .collect::<Vec<_>>();

        Ok(data)
    }

    pub async fn get_detailed_item(
        &self,
        id: IMDbId,
    ) -> Result<IMDbDetailedItem, IMDbScraperError> {
        let url = format!("https://imdb.com/title/{id}");
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|err| IMDbScraperError::Error(err.into()))?;

        if response.status().is_client_error() || response.status().is_server_error() {
            return Err(IMDbScraperError::DetailPageError(format!(
                "failed to get response: {}",
                response.status()
            )));
        }

        let data = match response.text().await {
            Ok(data) => self.parse_detailed_item_html(&data).await?,
            Err(err) => return Err(IMDbScraperError::Error(err.into())),
        };

        Ok(data)
    }

    pub async fn release_calendar(
        &self,
        media_type: IMDbMediaType,
    ) -> Result<Vec<IMDbReleaseCalendarItem>, IMDbScraperError> {
        let url = "https://www.imdb.com/calendar/";

        let media_type = match media_type {
            IMDbMediaType::Movie => "MOVIE",
            IMDbMediaType::TvShow => "TV", // maybe change to TV_EPISODE to get per episode releases rather than season/new series
        };

        let query = &[
            ("region", "US"), // Maybe allow customisable regions?
            ("type", media_type),
        ];
        let response = match self.client.get(url).query(query).send().await {
            Ok(response) => response,
            Err(err) => return Err(IMDbScraperError::Error(err.into())),
        };

        if response.status().is_client_error() || response.status().is_server_error() {
            return Err(IMDbScraperError::ReleaseCalendarError(format!(
                "Failed to get response: {}",
                response.status()
            )));
        }

        let data = match response.text().await {
            Ok(data) => data,
            Err(err) => return Err(IMDbScraperError::Error(err.into())),
        };

        Self::parse_release_calendar_html(&data)
    }

    pub async fn top_media(
        &self,
        media_type: IMDbMediaType,
        sort_by: IMDbSortBy,
    ) -> Result<Vec<IMDbTopMediaItem>, IMDbScraperError> {
        let url = match media_type {
            IMDbMediaType::Movie => "https://www.imdb.com/chart/moviemeter/",
            IMDbMediaType::TvShow => "https://www.imdb.com/chart/tvmeter/",
        };

        let response = match self.client.get(url).send().await {
            Ok(response) => response,
            Err(err) => return Err(IMDbScraperError::Error(err.into())),
        };

        if response.status().is_client_error() || response.status().is_server_error() {
            return Err(IMDbScraperError::TopMediaError(format!(
                "Failed to get response: {}",
                response.status()
            )));
        }

        let data = match response.text().await {
            Ok(data) => data,
            Err(err) => return Err(IMDbScraperError::Error(err.into())),
        };

        let mut output = Self::parse_top_media_html(&data)?;
        match sort_by {
            IMDbSortBy::Popularity => Ok(output),
            IMDbSortBy::ReleaseOrder => {
                output.sort_by(|a, b| b.item.release_date.cmp(&a.item.release_date));
                Ok(output)
            }
        }
    }

    fn find_next_data<T: for<'de> Deserialize<'de>>(html: &str) -> Result<T, IMDbScraperError> {
        let document = Html::parse_document(html);
        let next_script_selector =
            Selector::parse("script[id=\"__NEXT_DATA__\"][type=\"application/json\"]").unwrap();

        let data = match document.select(&next_script_selector).next() {
            Some(data) => match data.text().next() {
                Some(data) => data,
                None => {
                    return Err(IMDbScraperError::ReleaseCalendarError(
                        "Missing Text in __NEXT_DATA__".to_string(),
                    ));
                }
            },
            None => {
                return Err(IMDbScraperError::ReleaseCalendarError(
                    "Failed to find __NEXT_DATA__".to_string(),
                ));
            }
        };

        let data: T = match serde_json::from_str(data) {
            Ok(data) => data,
            Err(err) => return Err(IMDbScraperError::Error(err.into())),
        };

        Ok(data)
    }

    fn parse_release_calendar_html(
        html: &str,
    ) -> Result<Vec<IMDbReleaseCalendarItem>, IMDbScraperError> {
        let data: ReleaseCalendarNextData = Self::find_next_data(html)?;

        let items = data
            .props
            .page_props
            .groups
            .into_iter()
            .flat_map(|group| group.entries.into_iter().filter_map(|entry| entry.into()))
            .collect::<Vec<_>>();

        Ok(items)
    }

    fn parse_top_media_html(html: &str) -> Result<Vec<IMDbTopMediaItem>, IMDbScraperError> {
        let data: TopMediaNextData = Self::find_next_data(html)?;

        let data = data
            .props
            .page_props
            .page_data
            .chart_titles
            .edges
            .into_iter()
            .filter_map(|edge| edge.into())
            .collect::<Vec<_>>();

        Ok(data)
    }

    async fn parse_detailed_item_html(
        &self,
        html: &str,
    ) -> Result<IMDbDetailedItem, IMDbScraperError> {
        let data: IMDbPageNextData = Self::find_next_data(html)?;

        let data = data.props.page_props;

        let seasons = match data.above_the_fold_data.title_type.id {
            IMDbMediaType::Movie => None,
            IMDbMediaType::TvShow => match data.main_column_data.episodes {
                Some(seasons) => {
                    let season_futures = seasons.seasons.into_iter().map(|season| {
                        self.fetch_seasons_and_episodes(
                            data.above_the_fold_data.id.clone(),
                            season.number,
                        )
                    });

                    let mut seasons = futures::future::join_all(season_futures).await.into_iter();
                    let mut imdb_seasons = Vec::new();
                    while let Some(Ok(season)) = seasons.next() {
                        let season_number = match season.first() {
                            Some(season) => season.season,
                            None => continue,
                        };

                        let imdb_season = IMDbSeason {
                            season: season_number,
                            episodes: season.into_iter().map(|episode| episode.episode).collect(),
                        };
                        imdb_seasons.push(imdb_season);
                    }

                    if imdb_seasons.is_empty() {
                        None
                    } else {
                        Some(imdb_seasons)
                    }
                }
                None => {
                    return Err(IMDbScraperError::DetailPageError(
                        "Missing main column data for episodes".to_string(),
                    ));
                }
            },
        };

        let data = data.above_the_fold_data;
        let video = data
            .primary_videos
            .edges
            .into_iter()
            .find(|video| video.node.content_type.display_name.value == "Trailer");

        let video = match video {
            Some(video) => video
                .node
                .playback_urls
                .into_iter()
                .find(|url| url.video_mime_type == "MP4")
                .map(|url| url.url),
            None => None,
        };

        let release_date = match data.release_date.into() {
            Some(date) => date,
            None => return Err(IMDbScraperError::Error(format_err!("Invalid release date"))),
        };

        let item = IMDbDetailedItem {
            item: IMDbItem {
                id: data.id,
                title: data.title_text.text,
                year: data.release_year.year,
                image_url: Some(data.primary_image.url),
                _type: data.title_type.id,
            },
            plot: data.plot.plot_text.plain_text,
            runtime_seconds: data.runtime.seconds,
            video_url: video,
            release_date,
            seasons,
        };

        Ok(item)
    }

    async fn fetch_seasons_and_episodes(
        &self,
        imdb_id: IMDbId,
        season: i64,
    ) -> Result<Vec<TVSeasonEpisodeItem>, IMDbScraperError> {
        let url = format!("https://imdb.com/title/{imdb_id}/episodes");
        let payload = &[("season", season)];
        let response = self
            .client
            .get(url)
            .query(payload)
            .send()
            .await
            .map_err(|err| IMDbScraperError::Error(err.into()))?;

        if response.status().is_client_error() || response.status().is_server_error() {
            return Err(IMDbScraperError::DetailPageError(format!(
                "failed to get response: {}",
                response.status()
            )));
        }

        let data = match response.text().await {
            Ok(data) => Self::parse_episodes_html(&data)?,
            Err(err) => return Err(IMDbScraperError::Error(err.into())),
        };

        Ok(data)
    }

    fn parse_episodes_html(html: &str) -> Result<Vec<TVSeasonEpisodeItem>, IMDbScraperError> {
        let data: TVSeasonNextData = Self::find_next_data(html)?;
        Ok(data.props.page_props.content_data.section.episodes.items)
    }
}

impl Display for IMDbScraperError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            IMDbScraperError::SearchError(err) => f.write_fmt(format_args!("SearchError: {err}")),
            IMDbScraperError::TopMediaError(err) => {
                f.write_fmt(format_args!("TopMediaError: {err}"))
            }
            IMDbScraperError::ReleaseCalendarError(err) => {
                f.write_fmt(format_args!("ReleaseCalendarError: {err}"))
            }
            IMDbScraperError::DetailPageError(err) => {
                f.write_fmt(format_args!("DetailPageError: {err}"))
            }
            IMDbScraperError::Error(err) => f.write_fmt(format_args!("Error: {err}")),
        }
    }
}

impl core::error::Error for IMDbScraperError {}
impl From<SuggestionQueryData> for Option<IMDbItem> {
    fn from(value: SuggestionQueryData) -> Self {
        let image_url = match value.image {
            Some(image) => image.image_url,
            None => None,
        };

        let _type = match value._type {
            Some(_type) => match &*_type {
                "movie" => IMDbMediaType::Movie,
                "tvSeries" => IMDbMediaType::TvShow,
                _ => return None,
            },
            None => return None,
        };

        Some(IMDbItem {
            id: value.id,
            title: value.title,
            year: value.year.unwrap_or(0),
            image_url,
            _type,
        })
    }
}

#[derive(Debug, Deserialize)]
struct IMDbSearchQueryResponse {
    #[serde(rename = "d")]
    data: Vec<SuggestionQueryData>,
}

#[derive(Debug, Deserialize)]
pub struct SuggestionQueryData {
    #[serde(rename = "i")]
    image: Option<SuggestionQueryImage>,
    id: IMDbId,
    #[serde(rename = "l")]
    title: String,
    #[serde(rename = "qid")]
    _type: Option<String>,
    // #[serde(rename = "vt")]
    // _vt: Option<i64>,
    #[serde(rename = "y")]
    year: Option<i64>,
}
#[derive(Debug, Deserialize)]
struct SuggestionQueryImage {
    #[serde(rename = "imageUrl")]
    image_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReleaseCalendarNextData {
    #[serde(rename = "props")]
    props: ReleaseCalendarProps,
}

#[derive(Debug, Deserialize)]
struct ReleaseCalendarProps {
    #[serde(rename = "pageProps")]
    page_props: ReleaseCalendarPageProps,
}

#[derive(Debug, Deserialize)]
struct ReleaseCalendarPageProps {
    groups: Vec<ReleaseCalendarGroup>,
}

#[derive(Debug, Deserialize)]
struct ReleaseCalendarGroup {
    group: String,
    entries: Vec<ReleaseCalendarGroupEntry>,
}

#[derive(Debug, Deserialize)]
struct ReleaseCalendarGroupEntry {
    id: IMDbId,
    #[serde(rename = "titleText")]
    title_text: String,
    #[serde(rename = "titleType")]
    title_type: TitleType,
    #[serde(rename = "imageModel")]
    image_model: Option<Image>,
    #[serde(rename = "releaseDate")]
    release_date: String,
    #[serde(rename = "releaseYear")]
    release_year: Year,
}

impl From<ReleaseCalendarGroupEntry> for Option<IMDbReleaseCalendarItem> {
    fn from(value: ReleaseCalendarGroupEntry) -> Self {
        let release_date = match DateTime::from_str(&value.release_date) {
            Ok(date) => date,
            Err(err) => {
                warn!("Error parsing release date to DateTime: {}", err);
                return None;
            }
        };

        Some(IMDbReleaseCalendarItem {
            release_date,
            item: IMDbItem {
                id: value.id,
                title: value.title_text,
                year: value.release_year.year,
                image_url: value.image_model.map(|i_m| i_m.url),
                _type: value.title_type.id,
            },
        })
    }
}

#[derive(Debug, Deserialize)]
struct Image {
    url: String,
}

#[derive(Debug, Deserialize)]
struct Year {
    year: i64,
}

#[derive(Debug, Deserialize)]
struct TitleType {
    id: IMDbMediaType,
}

#[derive(Debug, Deserialize)]
struct TopMediaNextData {
    props: TopMediaNextDataProps,
}

#[derive(Debug, Deserialize)]
struct TopMediaNextDataProps {
    #[serde(rename = "pageProps")]
    page_props: TopMediaNextDataPageProps,
}

#[derive(Debug, Deserialize)]
struct TopMediaNextDataPageProps {
    #[serde(rename = "pageData")]
    page_data: TopMediaNextDataPageData,
}

#[derive(Debug, Deserialize)]
struct TopMediaNextDataPageData {
    #[serde(rename = "chartTitles")]
    chart_titles: TopMediaChartTitles,
}

#[derive(Debug, Deserialize)]
struct TopMediaChartTitles {
    edges: Vec<TopMediaChartTitleEdge>,
}

#[derive(Debug, Deserialize)]
struct TopMediaChartTitleEdge {
    #[serde(rename = "currentRank")]
    current_rank: i64,
    node: TopMediaChartTitleEdgeNode,
}

impl From<TopMediaChartTitleEdge> for Option<IMDbTopMediaItem> {
    fn from(value: TopMediaChartTitleEdge) -> Self {
        let release_date = value.node.release_date;
        let day = release_date.day.unwrap_or(28); // 28th is last safe day for each month (due to February)
        let release_date = Utc
            .with_ymd_and_hms(release_date.year, release_date.month, day, 0, 0, 0)
            .unwrap();

        Some(IMDbTopMediaItem {
            ranking: value.current_rank,
            item: IMDbReleaseCalendarItem {
                release_date,
                item: IMDbItem {
                    id: value.node.id,
                    title: value.node.title_text,
                    year: value.node.release_year.year,
                    image_url: Some(value.node.primary_image.url),
                    _type: value.node.title_type.id,
                },
            },
        })
    }
}

#[derive(Debug, Deserialize)]
struct TopMediaChartTitleEdgeNode {
    id: IMDbId,
    #[serde(rename = "titleText")]
    title_text: String,
    #[serde(rename = "titleType")]
    title_type: TitleType,
    #[serde(rename = "primaryImage")]
    primary_image: Image,
    #[serde(rename = "releaseYear")]
    release_year: Year,
    #[serde(rename = "releaseDate")]
    release_date: ReleaseDate,
    #[serde(rename = "meterRanking")]
    meter_ranking: MeterRanking,
}

#[derive(Debug, Deserialize)]
struct ReleaseDate {
    day: Option<u32>,
    month: u32,
    year: i32,
}

impl From<ReleaseDate> for Option<DateTime<Utc>> {
    fn from(value: ReleaseDate) -> Self {
        let datetime = NaiveDate::from_ymd_opt(value.year, value.month, value.day.unwrap_or(1))?
            .and_time(NaiveTime::MIN)
            .and_utc();
        Some(datetime)
    }
}

#[derive(Debug, Deserialize)]
struct MeterRanking {
    #[serde(rename = "currentRank")]
    current_rank: u64,
}

#[derive(Debug, Deserialize)]
struct IMDbPageNextData {
    props: IMDbPageNextDataProps,
}

#[derive(Debug, Deserialize)]
struct IMDbPageNextDataProps {
    #[serde(rename = "pageProps")]
    page_props: IMDbPagePageProps,
}

#[derive(Debug, Deserialize)]
struct IMDbPagePageProps {
    #[serde(rename = "aboveTheFoldData")]
    above_the_fold_data: IMDBAboveTheFoldData,
    #[serde(rename = "mainColumnData")]
    main_column_data: IMDbMainColumnData,
}

#[derive(Debug, Deserialize)]
struct IMDbMainColumnData {
    episodes: Option<IMDbMainColumnEpisodes>,
}

#[derive(Debug, Deserialize)]
struct IMDbMainColumnEpisodes {
    seasons: Vec<IMDbMainColumnSeason>,
}

#[derive(Debug, Deserialize)]
struct IMDbMainColumnSeason {
    number: i64,
}

#[derive(Debug, Deserialize)]
struct IMDBAboveTheFoldData {
    id: IMDbId,
    #[serde(rename = "titleText")]
    title_text: TitleText,
    #[serde(rename = "titleType")]
    title_type: TitleType,
    #[serde(rename = "originalTitleText")]
    original_title_text: TitleText,
    certificate: Certificate,
    #[serde(rename = "releaseYear")]
    release_year: ReleaseYear,
    #[serde(rename = "releaseDate")]
    release_date: ReleaseDate,
    runtime: Runtime,
    plot: Plot,
    #[serde(rename = "primaryImage")]
    primary_image: Image,
    #[serde(rename = "primaryVideos")]
    primary_videos: Videos,
}

#[derive(Debug, Deserialize)]
struct TitleText {
    text: String,
}

#[derive(Debug, Deserialize)]
struct Certificate {
    rating: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseYear {
    year: i64,
    #[serde(rename = "endYear")]
    end_year: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct Runtime {
    seconds: i64,
}

#[derive(Debug, Deserialize)]
struct Plot {
    #[serde(rename = "plotText")]
    plot_text: PlotText,
}

#[derive(Debug, Deserialize)]
struct PlotText {
    #[serde(rename = "plainText")]
    plain_text: String,
}

#[derive(Debug, Deserialize)]
struct Videos {
    edges: Vec<VideoEdge>,
}

#[derive(Debug, Deserialize)]
struct VideoEdge {
    node: VideoEdgeNode,
}

#[derive(Debug, Deserialize)]
struct VideoEdgeNode {
    #[serde(rename = "playbackURLS")]
    playback_urls: Vec<VideoEdgeNodePlayback>,
    #[serde(rename = "contentType")]
    content_type: VideoEdgeNodeType,
}

#[derive(Debug, Deserialize)]
struct VideoEdgeNodePlayback {
    url: String,
    #[serde(rename = "videoMimeType")]
    video_mime_type: String,
    #[serde(rename = "videoDefinition")]
    video_definition: String,
}

#[derive(Debug, Deserialize)]
struct VideoEdgeNodeType {
    #[serde(rename = "displayName")]
    display_name: VideoEdgeNodeTypeDisplayName,
}

#[derive(Debug, Deserialize)]
struct VideoEdgeNodeTypeDisplayName {
    value: String,
}

#[derive(Debug, Deserialize)]
struct TVSeasonNextData {
    props: TVSeasonProps,
}

#[derive(Debug, Deserialize)]
struct TVSeasonProps {
    #[serde(rename = "pageProps")]
    page_props: TVSeasonPageProps,
}

#[derive(Debug, Deserialize)]
struct TVSeasonPageProps {
    #[serde(rename = "contentData")]
    content_data: TVSeasonContentData,
}

#[derive(Debug, Deserialize)]
struct TVSeasonContentData {
    section: TVSeasonContentDataSection,
}

#[derive(Debug, Deserialize)]
struct TVSeasonContentDataSection {
    episodes: TVSeasonEpisodes,
}

#[derive(Debug, Deserialize)]
struct TVSeasonEpisodes {
    items: Vec<TVSeasonEpisodeItem>,
}

#[derive(Debug, Deserialize)]
struct TVSeasonEpisodeItem {
    id: IMDbId,
    #[serde(rename = "type")]
    _type: String,
    season: i64,
    episode: i64,
    #[serde(rename = "titleText")]
    title_text: String,
    #[serde(rename = "releaseDate")]
    release_date: ReleaseDate,
}

impl<'de> Deserialize<'de> for IMDbMediaType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(IMDbMediaTypeVisitor)
    }
}

struct IMDbMediaTypeVisitor;
impl<'de> Visitor<'de> for IMDbMediaTypeVisitor {
    type Value = IMDbMediaType;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("movie or tvShow")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        match v {
            "movie" => Ok(Self::Value::Movie),
            "tvSeries" => Ok(Self::Value::TvShow),
            value => Err(serde::de::Error::custom(format!("got {value}"))),
        }
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_str(&v)
    }
}
