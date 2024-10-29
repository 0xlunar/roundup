use std::ops::Not;

use anyhow::format_err;
use chrono::Local;
use log::error;
use rayon::prelude::*;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Proxy;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

pub struct IMDB {
    search_type: SearchType,
    proxy: Option<Proxy>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum SearchType {
    MoviePopular,
    MovieLatestRelease,
    TVPopular,
    TVLatestRelease,
    Watchlist,
    Downloads,
    Query(String),
}

#[derive(Debug, sqlx::Type, Serialize, Clone)]
#[sqlx(type_name = "item_type", rename_all = "lowercase")]
pub enum ItemType {
    Movie,
    TvShow,
}

#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct IMDBItem {
    pub id: String,
    pub title: String,
    pub year: i64,
    pub image_url: String,
    pub rating: String,
    pub runtime: Option<i64>,
    pub video_thumbnail_url: Option<String>,
    pub video_url: Option<String>,
    pub plot: Option<String>,
    pub popularity_rank: Option<i32>,
    pub release_order: Option<i32>,
    pub _type: ItemType,
    pub watchlist: bool,
    #[serde(skip_serializing)]
    pub created_at: chrono::DateTime<Local>,
    #[serde(skip_serializing)]
    pub updated_at: chrono::DateTime<Local>,
}

#[derive(Debug, Clone)]
pub struct IMDBEpisode {
    pub season: i32,
    pub episode: i32,
}

impl<'a> IMDB {
    pub fn new(search_type: SearchType, proxy: Option<Proxy>) -> IMDB {
        IMDB { search_type, proxy }
    }

    pub async fn search(&self) -> anyhow::Result<Vec<IMDBItem>> {
        if self.search_type == SearchType::Watchlist {
            return Err(format_err!("Invalid Search Type"));
        }

        let mut headers = HeaderMap::new();
        headers.insert("Accept", HeaderValue::from_static("*/*"));
        headers.insert("DNT", HeaderValue::from_static("1"));
        headers.insert(
            "Referer",
            HeaderValue::from_static("https://www.imdb.com/chart/moviemeter/"),
        );
        headers.insert(
            "Accept-Language",
            HeaderValue::from_static("en-US,en;q=0.9,en-AU;q=0.8"),
        );
        headers.insert("Cache-Control", HeaderValue::from_static("no-cache"));
        headers.insert("User-Agent", HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36"));

        let mut client = reqwest::ClientBuilder::new().default_headers(headers);
        client = match &self.proxy {
            Some(p) => client.proxy(p.to_owned()),
            None => client,
        };
        let client = client.build()?;

        let resp = match self.search_type {
            SearchType::Query(_) => {
                client
                    .get(self.search_type.to_url())
                    .header("Accept", "application/json")
                    .send()
                    .await?
            }
            _ => client.get(self.search_type.to_url()).send().await?,
        };

        let status = resp.status();
        let text = resp.text().await?;
        if status.is_server_error() || status.is_client_error() {
            return Err(format_err!(
                "Failed request: {:?}, Status: {}, Body: {}",
                self.search_type,
                status,
                text
            ));
        }

        match self.search_type {
            SearchType::Query(_) => self.parse_json(&text),
            _ => self.parse_html(&text),
        }
    }
    pub async fn search_tv_episodes(
        imdb_id: &str,
        proxy: Option<Proxy>,
        season: u32,
    ) -> anyhow::Result<Vec<IMDBEpisode>> {
        let mut client = reqwest::ClientBuilder::new().default_headers(Self::create_headers());
        client = match proxy.clone() {
            Some(p) => client.proxy(p),
            None => client,
        };
        let client = client.build()?;

        let query = match season {
            0 => Vec::new(),
            _ => vec![("season", season.to_string())],
        };

        let mut id = String::new();
        if imdb_id.starts_with("tt").not() {
            id.push_str("tt");
        }
        id.push_str(imdb_id);

        let url = format!("https://www.imdb.com/title/{}/episodes/", id);

        let resp = client.get(url).query(&query).send().await?;

        let status = resp.status();
        if status.is_server_error() || status.is_client_error() {
            return Err(format_err!("Failed request, Status: {}", status));
        }

        let text = resp.text().await?;

        let data = Self::scrape_html_for_data_tv(&text)?;

        let mut episodes = data
            .page_props
            .content_data
            .section
            .episodes
            .items
            .par_iter()
            .map(|e| IMDBEpisode::new(e.season.parse().unwrap(), e.episode.parse().unwrap()))
            .collect::<Vec<IMDBEpisode>>();

        if season.eq(&0) && data.page_props.content_data.section.seasons.len().gt(&0) {
            let mut seasons = data.page_props.content_data.section.seasons.iter();
            seasons.next(); // Skip first season

            for s in seasons {
                let season = match s.value.parse::<u32>() {
                    Ok(t) => t,
                    Err(e) => {
                        error!("{}", e);
                        continue;
                    }
                };
                let mut ep =
                    Box::pin(IMDB::search_tv_episodes(imdb_id, proxy.clone(), season)).await?;
                episodes.append(&mut ep);
            }
        }

        Ok(episodes)
    }

    pub async fn update_media_data(id: &str, proxy: Option<Proxy>) -> anyhow::Result<IMDBItem> {
        let mut client = reqwest::ClientBuilder::new()
            .use_rustls_tls()
            .default_headers(Self::create_headers());
        client = match proxy {
            Some(p) => client.proxy(p.clone()),
            None => client,
        };
        let client = client.build()?;

        let url = format!("https://www.imdb.com/title/{}/?ref_=ls_t_2", id);

        let resp = client.get(url).send().await?;

        let status = resp.status();
        if status.is_server_error() || status.is_client_error() {
            return Err(format_err!("Failed request, Status: {}", status));
        }

        let text = resp.text().await?;
        let data = Self::scrape_html_for_data(&text)?;

        let data = data.page_props.above_the_fold_data;

        let rating = match data.certificate {
            Some(t) => t.rating,
            None => "TBD".to_string(),
        };

        let runtime = data.runtime.map(|t| t.seconds);

        let plot = match data.plot {
            Some(t) => Some(t.plot_text.plain_text),
            None => None,
        };

        let _type = match data.can_have_episodes {
            true => ItemType::TvShow,
            false => ItemType::Movie,
        };

        let imdb_item = IMDBItem {
            id: data.id,
            title: data.title_text.text,
            year: data.release_year.year,
            image_url: data.primary_image.url,
            rating,
            runtime,
            video_thumbnail_url: None,
            video_url: None,
            plot,
            popularity_rank: None,
            release_order: None,
            _type,
            watchlist: false,
            created_at: Default::default(),
            updated_at: Default::default(),
        };

        Ok(imdb_item)
    }
    fn scrape_html_for_data(html: &str) -> anyhow::Result<IMDBNextDataResponse> {
        let html = Html::parse_document(html);
        let next_data = Selector::parse("#__NEXT_DATA__").unwrap();

        match html.select(&next_data).next() {
            Some(data_element) => match data_element.text().next() {
                Some(text) => {
                    let data: IMDBNextDataResponseProps = serde_json::from_str(text)?;
                    Ok(data.props)
                }
                None => Err(format_err!("Missing data")),
            },
            None => Err(format_err!("Failed to find IMDb metadata")),
        }
    }
    fn scrape_html_for_data_tv(html: &str) -> anyhow::Result<IMDBTVSeasonResponse> {
        let html = Html::parse_document(html);
        let next_data = Selector::parse("#__NEXT_DATA__").unwrap();

        match html.select(&next_data).next() {
            Some(data_element) => match data_element.text().next() {
                Some(text) => {
                    let data: IMDBTVSeasonResponseHTML = serde_json::from_str(text)?;
                    Ok(data.props)
                }
                None => Err(format_err!("Missing data")),
            },
            None => Err(format_err!("Failed to find IMDb metadata")),
        }
    }

    fn parse_json(&self, data: &str) -> anyhow::Result<Vec<IMDBItem>> {
        let resp_data: IMDBSuggestionQueryResponse = serde_json::from_str(data)?;

        let output = resp_data.data.par_iter().filter(|item| {
            ItemType::try_from(item._type.as_ref().unwrap_or(&"".to_string()).as_str()).is_ok()
                && item.year.is_some()
                && item.image.is_some()
        }).map(|item| {
            IMDBItem {
                id: item.id.to_string(),
                title: item.title.to_string(),
                year: *item.year.as_ref().unwrap(),
                image_url: item.image.as_ref().unwrap().image_url.as_ref().unwrap_or(&"https://upload.wikimedia.org/wikipedia/commons/thumb/a/ac/No_image_available.svg/300px-No_image_available.svg.png".to_string()).replace("._V1_", "._V1_UX200_CR0,4,200,300_"),
                rating: "TBD".to_string(),
                runtime: None,
                video_thumbnail_url: None,
                video_url: None,
                plot: None,
                popularity_rank: None, // This is Search Query only so always None
                release_order: None, // This is Search Query only so always None
                _type: ItemType::try_from(item._type.as_ref().unwrap().as_str()).unwrap(),
                watchlist: false,
                created_at: Local::now(),
                updated_at: Local::now(),
            }
        }).collect::<Vec<IMDBItem>>();

        Ok(output)
    }
    fn parse_html(&self, data: &str) -> anyhow::Result<Vec<IMDBItem>> {
        let html = Html::parse_document(data);

        let next_data_selector =
            Selector::parse("script[id=\"__NEXT_DATA__\"][type=\"application/json\"]").unwrap();

        let json_data = match html.select(&next_data_selector).next() {
            Some(d) => match d.text().next() {
                Some(t) => t,
                None => return Err(format_err!("__NEXT_DATA__ Missing Text")),
            },
            None => return Err(format_err!("__NEXT_DATA__ Missing Element")),
        };

        let data: IMDBMeterObject = serde_json::from_str(json_data)?;

        let output = data
            .props
            .page_props
            .page_data
            .chart_titles
            .edges
            .par_iter()
            .enumerate()
            .map(|(i, edge)| {
                let year = match &edge.node.release_year {
                    Some(t) => t.year,
                    None => 0,
                };

                IMDBItem {
                    id: edge.node.id.to_string(),
                    title: edge.node.title_text.text.to_string(),
                    year,
                    image_url: edge
                        .node
                        .primary_image
                        .url
                        .to_string()
                        .replace("._V1_", "._V1_UX200_CR0,4,200,300_"),
                    rating: match &edge.node.certificate {
                        Some(c) => c.rating.to_string(),
                        None => "TBD".to_string(),
                    },
                    runtime: None,
                    video_thumbnail_url: None,
                    video_url: None,
                    plot: None,
                    popularity_rank: match self.search_type {
                        SearchType::MoviePopular => Some((i + 1) as i32),
                        SearchType::TVPopular => Some((i + 1) as i32),
                        _ => None,
                    },
                    release_order: match self.search_type {
                        SearchType::MovieLatestRelease => Some((i + 1) as i32),
                        SearchType::TVLatestRelease => Some((i + 1) as i32),
                        _ => None,
                    },
                    _type: match self.search_type {
                        SearchType::MoviePopular => ItemType::Movie,
                        SearchType::MovieLatestRelease => ItemType::Movie,
                        SearchType::TVPopular => ItemType::TvShow,
                        SearchType::TVLatestRelease => ItemType::TvShow,
                        SearchType::Watchlist => unreachable!("Not a watchlist"),
                        SearchType::Downloads => unreachable!("Not a Download"),
                        SearchType::Query(_) => unreachable!("Queried and got HTML"),
                    },
                    watchlist: false,
                    created_at: Local::now(),
                    updated_at: Local::now(),
                }
            })
            .collect();

        Ok(output)
    }
    fn create_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("Accept", HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7"));
        headers.insert(
            "Referer",
            HeaderValue::from_static("https://www.imdb.com/chart/moviemeter/"),
        );
        headers.insert(
            "Accept-Language",
            HeaderValue::from_static("en-US,en;q=0.9,en-AU;q=0.8"),
        );
        headers.insert("Cache-Control", HeaderValue::from_static("no-cache"));
        headers.insert("pragma", HeaderValue::from_static("no-cache"));
        headers.insert("priority", HeaderValue::from_static("u=0, i"));
        headers.insert(
            "sec-ch-ua",
            HeaderValue::from_static(
                "\"Google Chrome\";v=\"129\", \"Not=A?Brand\";v=\"8\", \"Chromium\";v=\"129\"",
            ),
        );
        headers.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
        headers.insert(
            "sec-ch-ua-platform",
            HeaderValue::from_static("\"Windows\""),
        );
        headers.insert("sec-fetch-dest", HeaderValue::from_static("document"));
        headers.insert("sec-fetch-mode", HeaderValue::from_static("navigate"));
        headers.insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
        headers.insert("sec-fetch-user", HeaderValue::from_static("?1"));
        headers.insert("sec-gpc", HeaderValue::from_static("1"));
        headers.insert("upgrade-insecure-requests", HeaderValue::from_static("1"));
        headers.insert("User-Agent", HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36"));
        headers
    }
}

impl<'a> SearchType {
    fn to_url(&self) -> String {
        match self {
            SearchType::MoviePopular => {
                "https://www.imdb.com/chart/moviemeter/?sort=popularity%2Casc".to_string()
            }
            SearchType::MovieLatestRelease => {
                "https://www.imdb.com/chart/moviemeter/?sort=release_date%2Cdesc".to_string()
            }
            SearchType::TVPopular => {
                "https://www.imdb.com/chart/tvmeter/?sort=popularity%2Casc".to_string()
            }
            SearchType::TVLatestRelease => {
                "https://www.imdb.com/chart/tvmeter/?sort=release_date%2Cdesc".to_string()
            }
            SearchType::Watchlist => "".to_string(),
            SearchType::Downloads => "".to_string(),
            SearchType::Query(query) => format!(
                "https://v3.sg.media-imdb.com/suggestion/x/{}.json?includeVideos=1",
                query.trim().replace(" ", "%20")
            ),
        }
    }
}

impl IMDBEpisode {
    fn new(season: i32, episode: i32) -> Self {
        Self { season, episode }
    }
}

impl TryFrom<&str> for ItemType {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.eq_ignore_ascii_case("movie") || value.contains("film") {
            Ok(ItemType::Movie)
        } else if value.starts_with("tv") {
            Ok(ItemType::TvShow)
        } else {
            Err(format_err!("Invalid input: {}", value))
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct IMDBSuggestionQueryResponse {
    #[serde(rename = "d")]
    data: Vec<SuggestionQueryData>,
}

#[derive(Debug, Clone, Deserialize)]
struct SuggestionQueryData {
    #[serde(rename = "i")]
    image: Option<SuggestionQueryImage>,
    #[serde(rename = "id")]
    id: String,
    #[serde(rename = "l")]
    title: String,
    #[serde(rename = "qid")]
    _type: Option<String>,
    #[serde(rename = "vt")]
    _vt: Option<i64>,
    #[serde(rename = "y")]
    year: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
struct SuggestionQueryImage {
    #[serde(rename = "imageUrl")]
    image_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IMDBMeterObject {
    props: Props,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Props {
    page_props: PageProps,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageProps {
    page_data: PageData,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageData {
    chart_titles: ChartTitles,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChartTitles {
    edges: Vec<Edge>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Edge {
    node: Node,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    id: String,
    title_text: TitleText,
    primary_image: PrimaryImage,
    release_year: Option<ReleaseYear>,
    certificate: Option<Certificate>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TitleText {
    text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrimaryImage {
    url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseYear {
    year: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Certificate {
    rating: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IMDBTVSeasonResponseHTML {
    pub props: IMDBTVSeasonResponse,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IMDBTVSeasonResponse {
    pub page_props: IMDBTVSeasonPageProps,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IMDBTVSeasonPageProps {
    pub content_data: IMDBTVSeasonContentData,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IMDBTVSeasonContentData {
    pub section: IMDBTVSeasonSection,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IMDBTVSeasonSection {
    pub seasons: Vec<IMDBTVSeason>,
    pub episodes: IMDBTVSeasonEpisodes,
    pub current_season: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IMDBTVSeason {
    pub value: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IMDBTVSeasonEpisodes {
    pub items: Vec<IMDBTVSeasonItem>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IMDBTVSeasonItem {
    pub id: String,
    pub season: String,
    pub episode: String,
}

///////////
// Page Data JSON

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IMDBNextDataResponseProps {
    props: IMDBNextDataResponse,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IMDBNextDataResponse {
    page_props: IMDBNextDataPageProps,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IMDBNextDataPageProps {
    above_the_fold_data: IMDBNextDataAboveTheFoldData,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IMDBNextDataAboveTheFoldData {
    id: String,
    can_have_episodes: bool,
    title_text: TitleText,
    certificate: Option<Certificate>,
    release_year: ReleaseYear,
    runtime: Option<Runtime>,
    primary_image: PrimaryImage,
    plot: Option<IMDBNextDataPlot>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Runtime {
    seconds: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IMDBNextDataPlot {
    plot_text: IMDBNextDataPlotText,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IMDBNextDataPlotText {
    plain_text: String,
}
