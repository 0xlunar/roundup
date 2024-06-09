use std::ops::Not;

use actix_web::http::header::HeaderValue;
use anyhow::format_err;
use async_trait::async_trait;
use log::{debug, error, info};
use rayon::prelude::*;
use reqwest::{Client, ClientBuilder};
use reqwest::header::HeaderMap;
use scraper::{Html, Selector};

use crate::api::imdb::{IMDBEpisode, ItemType};
use crate::api::torrent::{MediaQuality, TorrentItem, TorrentSearch};

pub struct TheRARBG {
    client: Client,
}

impl TheRARBG {
    pub fn new() -> Box<Self> {
        let mut headers = HeaderMap::new();
        headers.insert("User-Agent", HeaderValue::from_static("roundup/1.0"));
        headers.insert("Accept", HeaderValue::from_static("application/json"));

        let client = ClientBuilder::new()
            .default_headers(headers)
            .build()
            .unwrap();

        Box::new(Self { client })
    }

    fn base_url(&self) -> &'static str {
        "https://therarbg.com/"
    }

    async fn fetch_query(&self, query: &str, page: u32) -> anyhow::Result<Option<String>> {
        let page = match page {
            0 => 1,
            _ => page,
        };

        let url = format!("{}get-posts/keywords:{}:category:Movies:category:TV:category:Anime:ncategory:XXX/?page={}", self.base_url(), query, page);

        let resp = self.client.get(url).send().await?;
        if resp.status().is_server_error() || resp.status().is_client_error() {
            return Err(format_err!("Failed to send request: {}", resp.status()));
        }

        if resp.url().as_str() == self.base_url() {
            return Ok(None);
        }

        let text = resp.text().await?;
        Ok(Some(text))
    }

    fn parse_search_table_html(
        &self,
        html: String,
        tv_episodes: Option<&Vec<IMDBEpisode>>,
    ) -> Vec<(
        String,
        MediaQuality,
        ItemType,
        Option<i32>,
        Option<i32>,
        u32,
    )> {
        let html = Html::parse_document(&html);
        let table_rows_selector = Selector::parse("tbody > tr").unwrap();
        let row_name_selector =
            Selector::parse(".cellName > div > a[href^=\"/post-detail/\"]").unwrap();
        let media_type_selector =
            Selector::parse("td.hideCell > a[href^=\"/get-posts/category:\"]").unwrap();
        let seeds_selector = Selector::parse("td[style=\"color: green\"]").unwrap();

        let mut urls = Vec::new();

        for row in html.select(&table_rows_selector) {
            let name = match row.select(&row_name_selector).next() {
                Some(t) => match t.text().next() {
                    Some(t) => t,
                    None => continue,
                },
                None => continue,
            };

            let media_type = match row.select(&media_type_selector).next() {
                Some(t) => match t.text().next() {
                    Some(t) => match t.trim() {
                        "TV" => ItemType::TvShow,
                        "Movies" => ItemType::Movie,
                        "Anime" => ItemType::TvShow, // Most likely?
                        _ => continue, // Unknown or category could contain either type
                    },
                    None => continue,
                },
                None => continue,
            };

            let seeds = match row.select(&seeds_selector).next() {
                // Check it has at least 1 seed
                Some(t) => match t.text().next() {
                    Some(t) => match t.trim().parse::<u32>() {
                        Ok(seeds) if seeds > 0 => seeds,
                        _ => continue,
                    },
                    None => continue,
                },
                None => continue,
            };

            let negative_keywords = ["hdcam", "hdts", "ts", "cam", "camrip", "telesync", "tsx"]; // I don't care for cams/telesyncs, update these if you like them.

            let split_name = name
                .split(' ')
                .map(|x| x.to_string())
                .collect::<Vec<String>>();
            if split_name
                .par_iter()
                .any(|x| negative_keywords.contains(&x.to_lowercase().as_str()))
                || name.to_lowercase().contains("hd ts")
            {
                continue;
            }

            let default = String::from("unknown");
            let quality = split_name
                .par_iter()
                .find_first(|word| word.ends_with("0p"))
                .unwrap_or(&default);
            let quality = match quality.as_str() {
                "480p" => MediaQuality::_480p,
                "720p" => MediaQuality::_720p,
                "1080p" => MediaQuality::_1080p,
                "2160p" => MediaQuality::_2160p,
                _ => continue,
            };

            let (season, episode) = match &tv_episodes {
                // Check if the torrent is an episode we are missing
                Some(episodes) => {
                    let name_lowercase = name.to_lowercase();
                    let (season, episode) = match split_name
                        .par_iter()
                        .find_first(|x| 
                            (x.starts_with('S') && x.contains('E')) || // Individual Episodes
                            ((x.as_str().eq("Season") || x.as_str().eq("season")) && !name_lowercase.contains("episode"))) // Season packs
                    {
                        Some(t) => {
                            if t.as_str().eq("Season") || t.as_str().eq("season") { // Season Packs
                                let mut peekable = split_name.iter().peekable();
                                let mut season_number = None;
                                while let Some(t) = peekable.next() {
                                    if t.as_str().eq("Season") || t.as_str().eq("season") {
                                        match peekable.peek() {
                                            Some(p) => match p.parse::<i32>() {
                                                Ok(s) => season_number = Some(s),
                                                Err(_) => continue,
                                            },
                                            None => break,
                                        }
                                    }
                                }
                                
                                if season_number.is_none() {
                                    continue;
                                }

                                (season_number, Some(-1))
                            } else { // Individual Episodes
                                match t.split_once('E') {
                                    Some((s, e)) => {
                                        let season =
                                            match s.strip_prefix('S').unwrap().parse::<i32>() {
                                                Ok(t) => t,
                                                Err(_) => continue,
                                            };

                                        let episode = match e.parse::<i32>() {
                                            Ok(t) => t,
                                            Err(_) => continue,
                                        };

                                        if episodes.par_iter().any(|missing_episode| {
                                            missing_episode.season == season
                                                && missing_episode.episode == episode
                                        }) {
                                            (Some(season), Some(episode))
                                        } else {
                                            continue;
                                        }
                                    }
                                    None => continue,
                                }
                            }
                        }
                        None => continue,
                    };

                    (season, episode)
                }
                None => (None, None),
            };

            let url = match row.select(&row_name_selector).next() {
                Some(t) => match t.value().attr("href") {
                    Some(t) => t,
                    None => continue,
                },
                None => continue,
            };

            urls.push((url.to_string(), quality, media_type, season, episode, seeds));
        }

        urls
    }

    async fn fetch_torrent_data(
        &self,
        imdb_id: String,
        url: String,
        media_quality: MediaQuality,
        item_type: ItemType,
        season: Option<i32>,
        episode: Option<i32>,
        seeds: u32,
    ) -> anyhow::Result<TorrentItem> {
        let url = format!("{}{}", self.base_url(), url);
        let resp = self.client.get(url).send().await?;
        if resp.status().is_server_error() || resp.status().is_client_error() {
            return Err(format_err!("Failed to send request: {}", resp.status()));
        }

        let text = resp.text().await?;
        let output = self.parse_torrent_page(
            imdb_id,
            text,
            media_quality,
            item_type,
            season,
            episode,
            seeds,
        )?;

        Ok(output)
    }

    fn parse_torrent_page(
        &self,
        imdb_id: String,
        html: String,
        media_quality: MediaQuality,
        item_type: ItemType,
        season: Option<i32>,
        episode: Option<i32>,
        seeds: u32,
    ) -> anyhow::Result<TorrentItem> {
        let html = Html::parse_document(&html);

        let name_selector = Selector::parse("div.postContL > h4.text-center.m-4").unwrap();
        let magnet_selector = Selector::parse("a[href^=\"magnet:?xt=urn:btih:\"]").unwrap();
        let row_selector = Selector::parse("tbody > tr").unwrap();
        let row_header_selector = Selector::parse("th").unwrap();
        let row_data_selector = Selector::parse("td").unwrap();

        let name = match html.select(&name_selector).next() {
            Some(t) => match t.text().next() {
                Some(t) => t.to_string(),
                None => return Err(format_err!("Missing Title")),
            },
            None => return Err(format_err!("Missing Title")),
        };

        let magnet_uri = match html.select(&magnet_selector).next() {
            Some(t) => match t.value().attr("href") {
                Some(t) => t.to_string(),
                None => return Err(format_err!("Missing Magnet")),
            },
            None => return Err(format_err!("Missing Magnet")),
        };

        for row in html.select(&row_selector) {
            match row.select(&row_header_selector).next() {
                Some(t) => match t.text().next() {
                    Some(t) => {
                        if t != "Language:" {
                            continue;
                        }
                    }
                    None => continue,
                },
                None => continue,
            };

            match row.select(&row_data_selector).next() {
                Some(t) => match t.text().next() {
                    Some(t) if t == "English" || t == "english" => {
                        return Ok(TorrentItem {
                            imdb_id,
                            name,
                            magnet_uri,
                            quality: media_quality,
                            _type: item_type,
                            season,
                            episode,
                            seeds: Some(seeds),
                        })
                    }
                    _ => break,
                },
                None => break,
            };
        }

        Err(format_err!("Torrent unparseable"))
    }
}
#[async_trait]
impl TorrentSearch for TheRARBG {
    async fn search(
        &self,
        search_term: String,
        imdb_id: Option<String>,
        tv_episodes: Option<Vec<IMDBEpisode>>,
    ) -> anyhow::Result<Vec<TorrentItem>> {
        let search = match imdb_id.clone() {
            Some(imdb_id) => {
                if imdb_id.starts_with("tt") {
                    imdb_id
                } else {
                    format!("tt{}", imdb_id)
                }
            }
            None => search_term.split(' ').collect::<Vec<&str>>().join("%20"),
        };

        let mut page = 1;
        let mut outputs = Vec::new();
        while let Ok(Some(text)) = self.fetch_query(&search, page).await {
            let mut output = self.parse_search_table_html(text, tv_episodes.as_ref());
            outputs.append(&mut output);
            page += 1;
        }

        if outputs.is_empty() {
            return Err(format_err!("No torrents available"));
        }

        let imdb_id = imdb_id.unwrap();
        // TODO: See if we can add Rayon into_par_iter() here.
        let tasks = outputs
            .into_iter()
            .map(|t| self.fetch_torrent_data(imdb_id.clone(), t.0, t.1, t.2, t.3, t.4, t.5));

        let tasks = futures::future::join_all(tasks).await;
        let mut torrents = Vec::new();

        for task in tasks {
            match task {
                Ok(t) => torrents.push(t),
                Err(e) => error!("Error fetching torrent data: {}", e),
            }
        }
        
        match tv_episodes {
            Some(tv_episodes) => {
                let episodes = tv_episodes.iter().map(|ep| (ep.season, ep.episode)).collect::<Vec<(i32,i32)>>();
                torrents.retain(|x| x.season.is_some() && x.episode.is_some() 
                    && (episodes.contains(&(x.season.unwrap(), x.episode.unwrap())) 
                        || episodes.contains(&(x.season.unwrap(), -1))
                    )
                );
                torrents.sort_by(|a, b| {
                    let a_s = a.season.as_ref().unwrap();
                    let b_s = b.season.as_ref().unwrap();

                    if a_s == b_s {
                        let a_e = a.episode.as_ref().unwrap();
                        let b_e = b.episode.as_ref().unwrap();
                        if a_e == b_e {
                            let qual_ordering = b.quality.cmp(&a.quality);
                            if qual_ordering.is_eq() {
                                b.seeds.as_ref().unwrap().cmp(a.seeds.as_ref().unwrap())
                            } else {
                                qual_ordering
                            }
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
                        b.episode.as_ref().unwrap() == a.episode.as_ref().unwrap()
                            && a.quality == b.quality
                            && a.seeds.as_ref().unwrap() < b.seeds.as_ref().unwrap()
                    } else {
                        false
                    }
                });
            },
            None => {
                torrents.sort_by(|a, b| {
                    let qual_ordering = b.quality.cmp(&a.quality);
                    if qual_ordering.is_eq() {
                        let a_seeds = a.seeds.as_ref().unwrap();
                        let b_seeds = b.seeds.as_ref().unwrap();

                        b_seeds.cmp(a_seeds)
                    } else {
                        qual_ordering
                    }
                });

                torrents.dedup_by(|a, b| {
                    a.quality == b.quality && a.seeds.as_ref().unwrap() < b.seeds.as_ref().unwrap()
                });
            },
        }
        Ok(torrents)
    }
}
