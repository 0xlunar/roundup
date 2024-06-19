use std::ops::Not;

use anyhow::format_err;
use rayon::prelude::*;
use regex::Regex;
use reqwest::{Client, ClientBuilder};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct Episode {
    pub season: i32,
    pub episode: i32,
}

#[derive(Debug, Clone)]
pub struct Plex {
    client: Client,
    token: String,
}

impl Plex {
    pub fn new() -> anyhow::Result<Self> {
        let token = Plex::get_plex_auth_token()?;

        let mut headers = HeaderMap::new();
        headers.insert("User-Agent", HeaderValue::from_static("roundup/1.0"));
        headers.insert("Accept", HeaderValue::from_static("application/json"));

        let client = ClientBuilder::new()
            .default_headers(headers)
            .build()
            .unwrap();

        Ok(Self { client, token })
    }

    #[cfg(target_os = "windows")]
    fn get_plex_auth_token() -> anyhow::Result<String> {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let plex_reg = hkcu.open_subkey("Software\\Plex, Inc.\\Plex Media Server")?;
        let token: String = plex_reg.get_value("PlexOnlineToken")?;

        Ok(token)
    }

    #[cfg(target_os = "linux")]
    fn get_plex_auth_token() -> anyhow::Result<String> {
        use std::fs;
        use std::path::PathBuf;
        use xmltree::Element;

        let path: PathBuf = "/var/lib/plexmediaserver/Library/Application Support/Plex Media Server/Preferences.xml".into();
        let xml_file = fs::read_to_string(path)?;
        let xml = Element::parse(xml_file.as_bytes())?;
        let token = xml.attributes.get("PlexOnlineToken").unwrap();

        Ok(token.to_string())
    }

    #[cfg(target_os = "macos")]
    fn get_plex_auth_token() -> anyhow::Result<String> {
        use plist;
        use std::path::PathBuf;

        #[derive(Deserialize)]
        struct MacOSPlexPlist {
            #[serde(rename = "PlexOnlineToken")]
            plex_online_token: String,
        }

        let path: PathBuf = "~/Library/Preferences/com.plexapp.plexmediaserver.plist".into();
        let plist_file: MacOSPlexPlist = plist::from_file(path)?;
        Ok(plist_file.plex_online_token)
    }

    pub async fn exists_in_library(
        &self,
        search_term: &str,
        exact_match: bool,
    ) -> anyhow::Result<bool> {
        let year_regexp = Regex::new(r"(\(\d{4}\))").unwrap();
        let title = match year_regexp.split(search_term).next() {
            Some(t) => t.trim(),
            None => search_term,
        };

        let year = match year_regexp.find(search_term) {
            Some(t) => t
                .as_str()
                .strip_prefix('(')
                .unwrap()
                .strip_suffix(')')
                .unwrap()
                .parse::<u32>()
                .unwrap(),
            None => 0,
        };

        let query = [
            ("query", title),
            ("X-Plex-Token", self.token.as_str()),
            ("limit", "100"),
            ("includeCollections", "1"),
            ("includeExternalMedia", "1"),
        ];
        let resp = self
            .client
            .get("http://127.0.0.1:32400/hubs/search")
            .query(&query)
            .send()
            .await?;

        let status = resp.status();
        if status.is_client_error() || status.is_server_error() {
            return Err(format_err!("Failed to check library"));
        }

        let data: PlexLibrarySearch = match resp.text().await {
            Ok(t) => serde_json::from_str(&t)?,
            Err(e) => return Err(e.into()),
        };

        let movie_hub = match data
            .media_container
            .hub
            .par_iter()
            .find_any(|h| h.title.eq("Movies"))
        {
            Some(d) => d,
            None => return Err(format_err!("Missing Movies hub")),
        };

        let meta = match &movie_hub.metadata {
            Some(t) => t,
            None => return Ok(false), // No movies
        };

        for metadata in meta {
            if !exact_match {
                if metadata.title.starts_with(title) && metadata.year.eq(&year) {
                    match metadata.media.first() {
                        Some(t) => match t.part.first() {
                            Some(p) => {
                                if p.file.is_empty().not() {
                                    return Ok(true);
                                }
                            }
                            None => continue,
                        },
                        None => continue,
                    }
                } else {
                    continue;
                }
            } else if metadata.title == title && metadata.year.eq(&year) {
                match metadata.media.first() {
                    Some(t) => match t.part.first() {
                        Some(p) => {
                            if p.file.is_empty().not() {
                                return Ok(true);
                            }
                        }
                        None => continue,
                    },
                    None => continue,
                }
            } else {
                continue;
            }
        }

        Ok(false)
    }

    pub async fn tvshow_exists_in_library(
        &self,
        search_term: &str,
    ) -> anyhow::Result<Vec<Episode>> {
        let year_regexp = Regex::new(r"(\(\d{4}\))").unwrap();
        let title = match year_regexp.split(&search_term).next() {
            Some(t) => t.trim(),
            None => &search_term,
        };

        let year = match year_regexp.find(&search_term) {
            Some(t) => t
                .as_str()
                .strip_prefix("(")
                .unwrap()
                .strip_suffix(")")
                .unwrap()
                .parse::<u32>()
                .unwrap(),
            None => 0,
        };

        let query = [
            ("query", title),
            ("X-Plex-Token", self.token.as_str()),
            ("limit", "100"),
            ("includeCollections", "1"),
            ("includeExternalMedia", "1"),
        ];
        let resp = self
            .client
            .get("http://127.0.0.1:32400/hubs/search")
            .query(&query)
            .send()
            .await?;

        let status = resp.status();
        if status.is_client_error() || status.is_server_error() {
            return Err(format_err!("Failed to check library"));
        }

        let data: PlexLibrarySearch = match resp.text().await {
            Ok(t) => serde_json::from_str(&t)?,
            Err(e) => return Err(e.into()),
        };

        let shows_hub = match data
            .media_container
            .hub
            .into_par_iter()
            .find_any(|h| h.title.as_str() == "Shows")
        {
            Some(t) => match t.metadata {
                Some(t) => t,
                None => return Ok(vec![]),
            },
            None => return Err(format_err!("Missing TV hub in query")),
        };

        for metadata in shows_hub {
            if metadata.title.starts_with(title) && metadata.year.eq(&year) {
                let available = self
                    .fetch_available_tvshow_children(&metadata.rating_key)
                    .await?;
                return Ok(available);
            } else {
                continue;
            }
        }

        Ok(vec![])
    }

    async fn fetch_available_tvshow_children(&self, show_id: &str) -> anyhow::Result<Vec<Episode>> {
        let query = [("X-Plex-Token", self.token.as_str())];
        let resp = self
            .client
            .get(format!(
                "http://127.0.0.1:32400/library/metadata/{}/allLeaves",
                show_id
            ))
            .query(&query)
            .send()
            .await?;
        let status = resp.status();
        if status.is_client_error() || status.is_server_error() {
            return Err(format_err!("Failed to check library"));
        }

        let data: PlexTVLibrarySearch = match resp.text().await {
            Ok(t) => serde_json::from_str(&t)?,
            Err(e) => return Err(e.into()),
        };

        let data = data
            .media_container
            .metadata
            .into_par_iter()
            .filter(|i| {
                i.media
                    .first()
                    .is_some_and(|x| x.part.first().is_some_and(|x| x.file.is_empty().not()))
            })
            .map(|i| Episode {
                season: i.parent_index,
                episode: i.index,
            })
            .collect::<Vec<Episode>>();

        Ok(data)
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlexLibrarySearch {
    #[serde(rename = "MediaContainer")]
    media_container: MediaContainer,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaContainer {
    size: i64,
    #[serde(rename = "Hub")]
    hub: Vec<Hub>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hub {
    title: String,
    #[serde(rename = "Metadata")]
    metadata: Option<Vec<PlexLibraryMetadata>>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlexLibraryMetadata {
    title: String,
    #[serde(default)]
    year: u32,
    rating_key: String,
    #[serde(rename = "Media", default)]
    media: Vec<MetadataMedia>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetadataMedia {
    #[serde(rename = "Part")]
    part: Vec<Part>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Part {
    file: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlexTVLibrarySearch {
    #[serde(rename = "MediaContainer")]
    media_container: PlexTVMediaContainer,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlexTVMediaContainer {
    #[serde(rename = "Metadata")]
    metadata: Vec<PlexTVMetadata>,
}
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlexTVMetadata {
    parent_index: i32, // Season Number
    index: i32,        // Episode Number
    #[serde(rename = "Media", default)]
    media: Vec<MetadataMedia>,
}
