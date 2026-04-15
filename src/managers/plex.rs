use crate::database::{imdb::IMDbDB, Database};
use crate::scrapers::{IMDbId, IMDbMediaType};
use actix_web::web::Data;
use anyhow::format_err;
use serde::Deserialize;
use std::fmt::{Display, Formatter};
use wreq::{Client, Response};

pub struct PlexManager {
    client: Data<Client>,
    database: Data<Database>,
    auth_token: String,
    url: String,
}

#[derive(Debug)]
pub enum PlexManagerError {
    MissingAuthToken(String),
    MediaQueryError(String),
    DeleteMediaError(String),
    Error(anyhow::Error),
}

pub enum PlexMediaQuery {
    Query {
        title: String,
        year: i64,
        imdb_id: IMDbId,
        media_type: IMDbMediaType,
    },
    IMDb(IMDbId),
}

impl PlexManager {
    pub fn new(
        client: Data<Client>,
        database: Data<Database>,
        url: String,
    ) -> Result<Self, PlexManagerError> {
        let auth_token = Self::find_local_auth_token()?;
        Ok(Self {
            client,
            database,
            auth_token,
            url,
        })
    }

    pub async fn find_media(
        &self,
        query: PlexMediaQuery,
    ) -> Result<PlexLibraryItemType, PlexManagerError> {
        let (title, year, imdb_id, media_type) = match query {
            PlexMediaQuery::Query {
                title,
                year,
                imdb_id,
                media_type,
            } => (title, year, imdb_id, media_type),
            PlexMediaQuery::IMDb(imdb_id) => {
                let imdb_db = IMDbDB::new(&self.database);
                let metadata = match imdb_db.get_item(imdb_id.clone()).await {
                    Ok(output) => match output {
                        Some(output) => output,
                        None => {
                            return Err(PlexManagerError::Error(format_err!("Item does not e")));
                        }
                    },
                    Err(err) => return Err(PlexManagerError::Error(err.into())),
                };

                (metadata.title, metadata.year, imdb_id, metadata._type)
            }
        };

        let payload_strs = &[
            ("X-Plex-Token", &*self.auth_token),
            ("type", media_type.as_payload_str()),
            ("includeGuids", "1"),
            ("title", &*title),
            ("guid", imdb_id.as_str()),
        ];

        let payload_nums = &[("year", year)];
        let response = match self
            .client
            .get(format!("{}/library/matches", self.url))
            .query(payload_strs)
            .query(payload_nums)
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => return Err(PlexManagerError::Error(err.into())),
        };

        if response.status().is_client_error() || response.status().is_server_error() {
            return Err(PlexManagerError::MediaQueryError(format!(
                "Failed to query media: {}",
                response.status()
            )));
        }

        let data = PlexLibraryMatchResponse::from_response(response).await?;

        if data.media_container.size > 0 {
            Err(PlexManagerError::MediaQueryError(
                "No results returned".to_string(),
            ))
        } else {
            let guid = PlexLibraryItemGuid {
                id: format!("imdb://{}", imdb_id.as_str()),
            };
            let mut item = match data
                .media_container
                .metadata
                .into_iter()
                .find(|item| item.get_guids().contains(&guid))
            {
                Some(item) => item,
                None => {
                    return Err(PlexManagerError::MediaQueryError(
                        "Queried media not returned in results".to_string(),
                    ));
                }
            };

            if let PlexLibraryItemType::TvShow(tv_show) = &mut item {
                let leaves = self.fetch_tv_show_leaves(&tv_show.rating_key).await?;
                tv_show.episodes = leaves;
            }

            Ok(item)
        }
    }
    pub async fn delete_media(&self, plex_id: &str) -> Result<(), PlexManagerError> {
        let payload = &[("X-Plex-Token", &*self.auth_token)];
        let response = match self
            .client
            .delete(format!("{}/library/metadata/{plex_id}", self.url))
            .query(&payload)
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => return Err(PlexManagerError::Error(err.into())),
        };

        match response.status().as_u16() {
            200 => Ok(()),
            400 => Err(PlexManagerError::DeleteMediaError(
                "Item(s) could not be deleted!".to_string(),
            )),
            _ => Err(PlexManagerError::DeleteMediaError(format!(
                "Failed to get response: {}",
                response.status()
            ))),
        }
    }

    async fn fetch_tv_show_leaves(
        &self,
        plex_id: &str,
    ) -> Result<Vec<PlexTvShowLeaf>, PlexManagerError> {
        let payload = &[("X-Plex-Token", &*self.auth_token), ("includeGuids", "1")];
        let response = match self
            .client
            .delete(format!("{}/library/metadata/{plex_id}/allLeaves", self.url))
            .query(&payload)
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => return Err(PlexManagerError::Error(err.into())),
        };

        if response.status().is_client_error() || response.status().is_server_error() {
            return Err(PlexManagerError::MediaQueryError(format!(
                "Failed to query media leaves: {}",
                response.status()
            )));
        }

        let data = PlexLibraryMatchResponse::from_response(response).await?;

        let data = data
            .media_container
            .metadata
            .into_iter()
            .map(|item| match item {
                PlexLibraryItemType::Movie(_) => {
                    unreachable!("Got movie type from allLeaves query")
                }
                PlexLibraryItemType::TvShow(_) => {
                    unreachable!("Got TvShow type from allLeaves query")
                }
                PlexLibraryItemType::TvShowLeaf(leaf) => leaf,
            })
            .collect::<Vec<_>>();

        Ok(data)
    }

    #[cfg(target_os = "windows")]
    fn find_local_auth_token() -> Result<String, PlexManagerError> {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let plex_reg = match hkcu.open_subkey("Software\\Plex, Inc.\\Plex Media Server") {
            Ok(key) => key,
            Err(err) => return Err(PlexManagerError::MissingAuthToken(format!("{err}"))),
        };
        let token: String = match plex_reg.get_value("PlexOnlineToken") {
            Ok(token) => token,
            Err(err) => return Err(PlexManagerError::MissingAuthToken(format!("{err}"))),
        };

        Ok(token)
    }

    #[cfg(target_os = "linux")]
    fn find_local_auth_token() -> Result<String, PlexManagerError> {
        use std::fs;
        use std::path::PathBuf;
        use xmltree::Element;

        let path: PathBuf = "/var/lib/plexmediaserver/Library/Application Support/Plex Media Server/Preferences.xml".into();
        let xml_file = match fs::read_to_string(path) {
            Ok(file) => file,
            Err(err) => return Err(PlexManagerError::MissingAuthToken(format!("{err}"))),
        };
        let xml = match Element::parse(xml_file.as_bytes()) {
            Ok(element) => element,
            Err(err) => return Err(PlexManagerError::MissingAuthToken(format!("{err}"))),
        };
        let token = match xml.attributes.get("PlexOnlineToken") {
            Some(token) => token,
            None => {
                return Err(PlexManagerError::MissingAuthToken(
                    "token attribute does not exist".to_string(),
                ));
            }
        };

        Ok(token.to_string())
    }

    #[cfg(target_os = "macos")]
    fn find_local_auth_token() -> Result<String, PlexManagerError> {
        use plist;
        use std::path::PathBuf;

        #[derive(Deserialize)]
        struct MacOSPlexPlist {
            #[serde(rename = "PlexOnlineToken")]
            plex_online_token: String,
        }

        let path: PathBuf = "~/Library/Preferences/com.plexapp.plexmediaserver.plist".into();
        let plist_file: MacOSPlexPlist = match plist::from_file(path) {
            Ok(file) => file,
            Err(err) => return Err(PlexManagerError::MissingAuthToken(format!("{err}"))),
        };
        Ok(plist_file.plex_online_token)
    }
}

impl Display for PlexManagerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PlexManagerError::MissingAuthToken(err) => {
                f.write_fmt(format_args!("MissingAuthToken: {err}"))
            }
            PlexManagerError::MediaQueryError(err) => {
                f.write_fmt(format_args!("MediaQueryError: {err}"))
            }
            PlexManagerError::DeleteMediaError(err) => {
                f.write_fmt(format_args!("DeleteMediaError: {err}"))
            }
            PlexManagerError::Error(err) => f.write_fmt(format_args!("error: {err}")),
        }
    }
}

impl core::error::Error for PlexManagerError {}

impl PlexLibraryMatchResponse {
    pub async fn from_response(
        response: Response,
    ) -> Result<PlexLibraryMatchResponse, PlexManagerError> {
        match response.bytes().await {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(data) => Ok(data),
                Err(err) => Err(PlexManagerError::Error(err.into())),
            },
            Err(err) => Err(PlexManagerError::Error(err.into())),
        }
    }
}

#[derive(Debug, Deserialize)]
struct PlexLibraryMatchResponse {
    #[serde(rename = "MediaContainer")]
    media_container: PlexLibraryMatchMediaContainer,
}

#[derive(Debug, Deserialize)]
struct PlexLibraryMatchMediaContainer {
    size: u64,
    #[serde(rename = "Metadata")]
    metadata: Vec<PlexLibraryItemType>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum PlexLibraryItemType {
    Movie(PlexLibraryMovieItem),
    TvShow(PlexLibraryTvShowItem),
    TvShowLeaf(PlexTvShowLeaf),
}

impl PlexLibraryItemType {
    pub fn get_guids(&self) -> &[PlexLibraryItemGuid] {
        match self {
            PlexLibraryItemType::Movie(movie) => &movie.guid,
            PlexLibraryItemType::TvShow(tv) => &tv.guid,
            PlexLibraryItemType::TvShowLeaf(episode) => &episode.guid, // Will be unused but just incase
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PlexLibraryMovieItem {
    pub rating_key: String,
    pub title: String,
    pub year: u32,
    #[serde(rename = "Media")]
    pub media: Vec<PlexLibraryItemMedia>,
    #[serde(rename = "Guid")]
    guid: Vec<PlexLibraryItemGuid>,
}

#[derive(Debug, Deserialize)]
pub struct PlexLibraryTvShowItem {
    pub rating_key: String,
    pub title: String,
    pub year: u32,
    #[serde(skip)]
    pub episodes: Vec<PlexTvShowLeaf>,
    #[serde(rename = "Guid")]
    guid: Vec<PlexLibraryItemGuid>,
}

#[derive(Debug, Deserialize)]
pub struct PlexLibraryItemMedia {
    pub id: u64,
    pub container: String, // File extension
    #[serde(rename = "videoResolution")]
    pub video_resolution: String,
    #[serde(rename = "Part")]
    pub part: Vec<PlexLibraryItemMediaPart>,
}

#[derive(Debug, Deserialize)]
pub struct PlexLibraryItemMediaPart {
    pub id: u64,
    pub key: String,
    pub duration: u64, // In Milliseconds
    pub file: String,
    pub size: u64,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct PlexLibraryItemGuid {
    id: String,
}

#[derive(Debug, Deserialize)]
pub struct PlexTvShowLeaf {
    pub rating_key: String,
    pub title: String,
    #[serde(rename = "index")]
    pub episode: u64,
    #[serde(rename = "parentIndex")]
    pub season: u64,
    #[serde(rename = "Media")]
    pub media: Vec<PlexLibraryItemMedia>,
    #[serde(rename = "Guid")]
    guid: Vec<PlexLibraryItemGuid>,
}
