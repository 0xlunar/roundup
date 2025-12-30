use crate::database::Database;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use log::warn;
use scraper::{Html, Selector};
use serde::Deserialize;
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
pub enum IMDbMediaType {
    Movie,
    TvShow,
}

#[derive(Debug, Clone)]
pub enum IMDbSortBy {
    Popularity,
    ReleaseOrder,
}

pub struct IMDbItem {
    id: String,
    name: String,
    year: u64,
    image_url: Option<String>,
    _type: IMDbMediaType,
}

pub struct IMDbReleaseCalendarItem {
    release_date: chrono::DateTime<Utc>,
    item: IMDbItem,
}

pub struct IMDbTopMediaItem {
    ranking: u64,
    item: IMDbReleaseCalendarItem,
}

#[derive(Debug)]
pub enum IMDbScraperError {
    SearchError(String),
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

    fn parse_release_calendar_html(
        html: &str,
    ) -> Result<Vec<IMDbReleaseCalendarItem>, IMDbScraperError> {
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

        let data: ReleaseCalendarNextData = match serde_json::from_str(data) {
            Ok(data) => data,
            Err(err) => return Err(IMDbScraperError::Error(err.into())),
        };

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
        let document = Html::parse_document(html);
        let next_script_selector =
            Selector::parse("script[id=\"__NEXT_DATA__\"][type=\"application/json\"]").unwrap();

        let data = match document.select(&next_script_selector).next() {
            Some(data) => match data.text().next() {
                Some(data) => data,
                None => {
                    return Err(IMDbScraperError::TopMediaError(
                        "Missing Text in __NEXT_DATA__".to_string(),
                    ));
                }
            },
            None => {
                return Err(IMDbScraperError::TopMediaError(
                    "Failed to find __NEXT_DATA__".to_string(),
                ));
            }
        };

        let data: TopMediaNextData = match serde_json::from_str(data) {
            Ok(data) => data,
            Err(err) => return Err(IMDbScraperError::Error(err.into())),
        };

        let data = data.props
            .page_props
            .page_data
            .chart_titles
            .edges
            .into_iter()
            .filter_map(|edge| edge.into())
            .collect::<Vec<_>>();

        Ok(data)
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
            name: value.title,
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
    id: String,
    #[serde(rename = "l")]
    title: String,
    #[serde(rename = "qid")]
    _type: Option<String>,
    // #[serde(rename = "vt")]
    // _vt: Option<i64>,
    #[serde(rename = "y")]
    year: Option<u64>,
}
#[derive(Debug, Deserialize)]
struct SuggestionQueryImage {
    #[serde(rename = "imageUrl")]
    image_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReleaseCalendarNextData {
    props: ReleaseCalendarProps,
}

#[derive(Debug, Deserialize)]
struct ReleaseCalendarProps {
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
    id: String,
    #[serde(rename = "titleText")]
    title_text: String,
    #[serde(rename = "titleType")]
    title_type: TitleType,
    #[serde(rename = "imageModel")]
    image_model: Option<Image>,
    release_date: String,
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

        let _type = match &*value.title_type.id {
            "movie" => IMDbMediaType::Movie,
            "tvSeries" => IMDbMediaType::TvShow,
            _ => return None,
        };

        Some(IMDbReleaseCalendarItem {
            release_date,
            item: IMDbItem {
                id: value.id,
                name: value.title_text,
                year: value.release_year.year,
                image_url: value.image_model.map(|i_m| i_m.url),
                _type,
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
    year: u64,
}

#[derive(Debug, Deserialize)]
struct TitleType {
    id: String,
}

#[derive(Debug, Deserialize)]
struct TopMediaNextData {
    props: TopMediaNextDataProps,
}

#[derive(Debug, Deserialize)]
struct TopMediaNextDataProps {
    page_props: TopMediaNextDataPageProps,
}

#[derive(Debug, Deserialize)]
struct TopMediaNextDataPageProps {
    page_data: TopMediaNextDataPageData,
}

#[derive(Debug, Deserialize)]
struct TopMediaNextDataPageData {
    chart_titles: TopMediaChartTitles,
}

#[derive(Debug, Deserialize)]
struct TopMediaChartTitles {
    edges: Vec<TopMediaChartTitleEdge>,
}

#[derive(Debug, Deserialize)]
struct TopMediaChartTitleEdge {
    current_rank: u64,
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
                    id: "".to_string(),
                    name: "".to_string(),
                    year: 0,
                    image_url: None,
                    _type: IMDbMediaType::Movie,
                },
            },
        })
    }
}

#[derive(Debug, Deserialize)]
struct TopMediaChartTitleEdgeNode {
    id: String,
    title_text: String,
    title_type: TitleType,
    primary_image: Image,
    release_year: Year,
    release_date: ReleaseDate,
    meter_ranking: MeterRanking,
}

#[derive(Debug, Deserialize)]
struct ReleaseDate {
    day: Option<u32>,
    month: u32,
    year: i32,
}

#[derive(Debug, Deserialize)]
struct MeterRanking {
    current_rank: u64,
}
