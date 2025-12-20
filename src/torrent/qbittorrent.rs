use crate::torrent::{
    ProcessableTorrentState, TorrentClient, TorrentClientError, TorrentIdentifier, TorrentInfo,
};
use async_trait::async_trait;
use serde::Deserialize;
use wreq::Client;
use wreq::multipart::Form;

pub struct QBittorrent {
    client: Client,
    host: String,
    credentials: QBittorrentCredentials,
}

pub struct QBittorrentCredentials {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
pub struct QBittorrentTorrentContents {
    pub index: u64,
    pub name: String,
    pub size: u64,
    pub progress: f64,
    pub priority: u8,
}

#[derive(Debug, Deserialize)]
pub struct QBittorrentTorrentInfo {
    pub hash: String,
    pub name: String,
    pub progress: f64,
    pub size: u64,
    pub state: QBittorrentTorrentState,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QBittorrentTorrentState {
    Error,
    MissingFiles,
    Uploading,
    PausedUP,
    QueuedUP,
    StalledUP,
    CheckingUP,
    ForcedUp,
    Allocating,
    Downloading,
    MetaDL,
    PausedDL,
    QueuedDL,
    StalledDL,
    CheckingDL,
    ForcedDL,
    CheckingResumeData,
    Moving,
    Unknown,
}

pub struct QBittorrentTorrentUpdateAdditionalParams {
    id: String,
    priority: u8,
}

impl QBittorrent {
    pub fn new(client: Client, host: String, credentials: QBittorrentCredentials) -> Self {
        Self {
            client,
            host,
            credentials,
        }
    }
}

impl QBittorrentCredentials {
    pub fn new(username: String, password: String) -> Self {
        Self { username, password }
    }
}

macro_rules! send_hashes_query {
    ($self:expr, $endpoint:literal, $identifier:expr, $error_type:ident) => {{
        // let hash = match $identifier {
        //     TorrentIdentifier::Hash(hash) => hash,
        //     TorrentIdentifier::Magnet(magnet) => match extract_hash_from_magnet(magnet) {
        //         Some(hash) => hash,
        //         None => {
        //             return Err(TorrentClientError::$error_type(
        //                 "Failed to extract hash from magnet".to_string(),
        //             ));
        //         }
        //     },
        // };

        let hash = match $identifier.to_hash() {
            Some(hash) => hash,
            None => {
                return Err(TorrentClientError::$error_type(
                    "Failed to extract hash".to_string(),
                ));
            }
        };

        let response = match $self
            .client
            .get(format!("{}{}", $self.host, $endpoint))
            .query(&[("hashes", hash)])
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                return Err(TorrentClientError::$error_type(format!(
                    "Failed to send request: {err}"
                )));
            }
        };

        if response.status().is_success() {
            Ok(true)
        } else {
            Err(TorrentClientError::$error_type(format!(
                "Failed to update torrent state: {}",
                response.status()
            )))
        }
    }};
}

#[async_trait]
impl TorrentClient for QBittorrent {
    type TorrentType<'a> = QBittorrentTorrentInfo;
    type TorrentContentsType<'a> = Vec<QBittorrentTorrentContents>;
    type TorrentUpdateAdditionalArguments = QBittorrentTorrentUpdateAdditionalParams;

    async fn connect(&self) -> Result<bool, TorrentClientError> {
        let response = self
            .client
            .get(format!("{}/api/v2/auth/login", &self.host))
            .query(&[
                ("username", &*self.credentials.username),
                ("password", &*self.credentials.password),
            ])
            .header("Referer", &*self.host)
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(err) => {
                return Err(TorrentClientError::ClientConnectError(format!(
                    "Failed to send request: {err}"
                )));
            }
        };
        if response.status().as_u16() == 403 {
            return Err(TorrentClientError::ClientConnectError(
                "Too many attempts".to_string(),
            ));
        }

        if response.cookies().any(|cookie| cookie.name() == "SID") {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn create_torrent(
        &self,
        identifier: TorrentIdentifier,
    ) -> Result<bool, TorrentClientError> {
        let torrent = match identifier {
            TorrentIdentifier::Hash(_) => {
                return Err(TorrentClientError::CreateTorrentError(
                    "Hash Identifier currently not implemented!".to_string(),
                ));
            }
            TorrentIdentifier::Magnet(magnet) => magnet,
        };
        let form = Form::new().text("urls", torrent.to_string());

        let response = self
            .client
            .post(format!("{}/api/v2/torrents/add", &self.host))
            .header("Referer", &*self.host)
            .multipart(form)
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(err) => {
                return Err(TorrentClientError::CreateTorrentError(format!(
                    "Failed to send request: {err}"
                )));
            }
        };
        if response.status().as_u16() == 415 {
            return Err(TorrentClientError::CreateTorrentError(
                "Torrent file not valid".to_string(),
            ));
        }

        Ok(true) // TODO: Find a confirmation if actually successful
    }

    async fn update_torrent(
        &self,
        identifier: TorrentIdentifier,
        additional: Self::TorrentUpdateAdditionalArguments,
    ) -> Result<bool, TorrentClientError> {
        let hash = match identifier.to_hash() {
            Some(hash) => hash,
            None => {
                return Err(TorrentClientError::UpdateTorrentError(
                    "Failed to extract hash".to_string(),
                ));
            }
        };
        let form = Form::new()
            .text("hash", hash.to_string())
            .text("id", additional.id)
            .text("priority", additional.priority.to_string());

        let response = self
            .client
            .post(format!("{}/api/v2/torrents/filePrio", &self.host))
            .header("Referer", &*self.host)
            .multipart(form)
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(err) => {
                return Err(TorrentClientError::UpdateTorrentError(format!(
                    "Failed to send request: {err}"
                )));
            }
        };
        if response.status().as_u16() == 415 {
            return Err(TorrentClientError::UpdateTorrentError(
                "Torrent file not valid".to_string(),
            ));
        }

        if response.status().as_u16() == 415 {
            return Err(TorrentClientError::UpdateTorrentError(
                "Torrent file not valid".to_string(),
            ));
        }

        if response.status().as_u16() == 415 {
            return Err(TorrentClientError::UpdateTorrentError(
                "Torrent file not valid".to_string(),
            ));
        }

        if response.status().as_u16() == 415 {
            return Err(TorrentClientError::UpdateTorrentError(
                "Torrent file not valid".to_string(),
            ));
        }

        match response.status().as_u16() {
            400 => Err(TorrentClientError::UpdateTorrentError(
                "Priority is invalid, or at least one file id is not a valid integer".to_string(),
            )),
            404 => Err(TorrentClientError::UpdateTorrentError(
                "Torrent hash was not found".to_string(),
            )),
            409 => Err(TorrentClientError::UpdateTorrentError(
                "Torrent metadata hasn't downloaded yet, or at least one file id was not found"
                    .to_string(),
            )),
            200 => Ok(true),
            _ => Err(TorrentClientError::UpdateTorrentError(format!(
                "Failed to make request: {}",
                response.status()
            ))),
        }
    }

    async fn pause_torrent(
        &self,
        identifier: TorrentIdentifier,
    ) -> Result<bool, TorrentClientError> {
        send_hashes_query!(self, "/api/v2/torrents/stop", identifier, PauseTorrentError)
    }

    async fn resume_torrent(
        &self,
        identifier: TorrentIdentifier,
    ) -> Result<bool, TorrentClientError> {
        send_hashes_query!(
            self,
            "/api/v2/torrents/resume",
            identifier,
            ResumeTorrentError
        )
    }

    async fn delete_torrent(
        &self,
        identifier: TorrentIdentifier,
        delete_file: bool,
    ) -> Result<bool, TorrentClientError> {
        send_hashes_query!(
            self,
            "/api/v2/torrents/delete",
            identifier,
            DeleteTorrentError
        )
    }

    async fn reannounce_torrent(
        &self,
        identifier: TorrentIdentifier,
    ) -> Result<bool, TorrentClientError> {
        send_hashes_query!(
            self,
            "/api/v2/torrents/reannounce",
            identifier,
            ReannounceTorrentError
        )
    }

    async fn view_torrent_contents(
        &self,
        identifier: TorrentIdentifier,
    ) -> Result<Self::TorrentContentsType<'_>, TorrentClientError> {
        let hash = match identifier.to_hash() {
            Some(hash) => hash,
            None => {
                return Err(TorrentClientError::ViewTorrentError(
                    "Failed to get hash".to_string(),
                ));
            }
        };

        let response = self
            .client
            .get(format!("{}/api/v2/torrents/files", &self.host))
            .header("Referer", &*self.host)
            .query(&[("hash", hash)])
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(err) => {
                return Err(TorrentClientError::ViewTorrentError(format!(
                    "Failed to send request: {err}"
                )));
            }
        };
        if response.status().as_u16() == 404 {
            return Err(TorrentClientError::ViewTorrentError(
                "Torrent hash not found".to_string(),
            ));
        }

        if !response.status().is_success() {
            return Err(TorrentClientError::ViewTorrentError(format!(
                "Error occurred with status code: {}",
                response.status()
            )));
        }

        let data = match response.bytes().await {
            Ok(bytes) => match serde_json::from_slice::<Self::TorrentContentsType<'_>>(&bytes) {
                Ok(data) => data,
                Err(err) => {
                    return Err(TorrentClientError::ViewTorrentError(format!(
                        "Error deserialising bytes: {}",
                        err
                    )));
                }
            },
            Err(err) => {
                return Err(TorrentClientError::ViewTorrentError(format!(
                    "Error parsing bytes: {}",
                    err
                )));
            }
        };

        Ok(data)
    }

    async fn view_all_torrents(&self) -> Result<Vec<Self::TorrentType<'_>>, TorrentClientError> {
        let response = self
            .client
            .get(format!("{}/api/v2/torrents/info", &self.host))
            .header("Referer", &*self.host)
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(err) => {
                return Err(TorrentClientError::ViewTorrentError(format!(
                    "Failed to send request: {err}"
                )));
            }
        };

        if !response.status().is_success() {
            return Err(TorrentClientError::ViewTorrentError(format!(
                "Error occurred with status code: {}",
                response.status()
            )));
        }

        let data = match response.bytes().await {
            Ok(bytes) => match serde_json::from_slice::<Vec<Self::TorrentType<'_>>>(&bytes) {
                Ok(data) => data,
                Err(err) => {
                    return Err(TorrentClientError::ViewTorrentError(format!(
                        "Error deserialising bytes: {}",
                        err
                    )));
                }
            },
            Err(err) => {
                return Err(TorrentClientError::ViewTorrentError(format!(
                    "Error parsing bytes: {}",
                    err
                )));
            }
        };

        Ok(data)
    }
}

impl TorrentInfo for QBittorrentTorrentInfo {
    fn get_id(&self) -> &str {
        &*self.hash
    }

    fn get_state(&self) -> ProcessableTorrentState {
        match self.state {
            QBittorrentTorrentState::Error => ProcessableTorrentState::Other("Torrent Errored"),
            QBittorrentTorrentState::MissingFiles => {
                ProcessableTorrentState::Other("Torrent missing files")
            }
            QBittorrentTorrentState::Uploading => ProcessableTorrentState::Seeding,
            QBittorrentTorrentState::PausedUP => ProcessableTorrentState::Finished,
            QBittorrentTorrentState::QueuedUP => ProcessableTorrentState::Seeding,
            QBittorrentTorrentState::StalledUP => ProcessableTorrentState::Stalled,
            QBittorrentTorrentState::CheckingUP => {
                ProcessableTorrentState::Other("Checking torrent for upload")
            }
            QBittorrentTorrentState::ForcedUp => ProcessableTorrentState::Seeding,
            QBittorrentTorrentState::Allocating => {
                ProcessableTorrentState::Other("Allocating space for torrent")
            }
            QBittorrentTorrentState::Downloading => {
                ProcessableTorrentState::Downloading(Some(self.progress))
            }
            QBittorrentTorrentState::MetaDL => {
                ProcessableTorrentState::Downloading(Some(self.progress))
            }
            QBittorrentTorrentState::PausedDL => ProcessableTorrentState::Paused,
            QBittorrentTorrentState::QueuedDL => ProcessableTorrentState::Paused,
            QBittorrentTorrentState::StalledDL => ProcessableTorrentState::Stalled,
            QBittorrentTorrentState::CheckingDL => {
                ProcessableTorrentState::Other("Checking torrent for download")
            }
            QBittorrentTorrentState::ForcedDL => {
                ProcessableTorrentState::Downloading(Some(self.progress))
            }
            QBittorrentTorrentState::CheckingResumeData => {
                ProcessableTorrentState::Other("Checking torrent to resume")
            }
            QBittorrentTorrentState::Moving => ProcessableTorrentState::Other("Moving torrent"),
            QBittorrentTorrentState::Unknown => {
                ProcessableTorrentState::Other("Unknown issue occurred")
            }
        }
    }

    fn get_size_in_bytes(&self) -> Option<u64> {
        Some(self.size)
    }
}
