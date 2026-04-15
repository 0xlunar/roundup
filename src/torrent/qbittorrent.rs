use crate::torrent::{
    ProcessableTorrentState, TorrentClient, TorrentClientError, TorrentContentInfo,
    TorrentIdentifier, TorrentInfo,
};
use actix_web::web::Data;
use async_trait::async_trait;
use itertools::Itertools;
use serde::Deserialize;
use wreq::multipart::Form;
use wreq::Client;

pub struct QBittorrent {
    client: Data<Client>,
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
    pub size: i64,
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

impl QBittorrent {
    pub fn new(client: Data<Client>, host: String, credentials: QBittorrentCredentials) -> Self {
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
    type TorrentContentType<'a> = QBittorrentTorrentContents;

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

    async fn update_file_priority(
        &self,
        identifier: TorrentIdentifier,
        files: Vec<<Self::TorrentContentType<'_> as TorrentContentInfo>::FileIdType>,
    ) -> Result<bool, TorrentClientError> {
        let hash = match identifier.to_hash() {
            Some(hash) => hash,
            None => {
                return Err(TorrentClientError::UpdateFilePriorityError(
                    "Failed to extract hash".to_string(),
                ));
            }
        };

        let form = Form::new()
            .text("hash", hash.to_string())
            .text("id", files.iter().join("|"))
            .text("priority", 0.to_string());

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
                return Err(TorrentClientError::UpdateFilePriorityError(format!(
                    "Failed to send request: {err}"
                )));
            }
        };
        if response.status().as_u16() == 415 {
            return Err(TorrentClientError::UpdateFilePriorityError(
                "Torrent file not valid".to_string(),
            ));
        }

        if response.status().as_u16() == 415 {
            return Err(TorrentClientError::UpdateFilePriorityError(
                "Torrent file not valid".to_string(),
            ));
        }

        if response.status().as_u16() == 415 {
            return Err(TorrentClientError::UpdateFilePriorityError(
                "Torrent file not valid".to_string(),
            ));
        }

        if response.status().as_u16() == 415 {
            return Err(TorrentClientError::UpdateFilePriorityError(
                "Torrent file not valid".to_string(),
            ));
        }

        match response.status().as_u16() {
            400 => Err(TorrentClientError::UpdateFilePriorityError(
                "Priority is invalid, or at least one file id is not a valid integer".to_string(),
            )),
            404 => Err(TorrentClientError::UpdateFilePriorityError(
                "Torrent hash was not found".to_string(),
            )),
            409 => Err(TorrentClientError::UpdateFilePriorityError(
                "Torrent metadata hasn't downloaded yet, or at least one file id was not found"
                    .to_string(),
            )),
            200 => Ok(true),
            _ => Err(TorrentClientError::UpdateFilePriorityError(format!(
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
        _: bool,
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
    ) -> Result<Vec<Self::TorrentContentType<'_>>, TorrentClientError> {
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
            Ok(bytes) => {
                match serde_json::from_slice::<Vec<Self::TorrentContentType<'_>>>(&bytes) {
                    Ok(data) => data,
                    Err(err) => {
                        return Err(TorrentClientError::ViewTorrentError(format!(
                            "Error deserialising bytes: {}",
                            err
                        )));
                    }
                }
            }
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
        &self.hash
    }

    fn as_identifier(&self) -> TorrentIdentifier {
        TorrentIdentifier::new_hash(&self.hash)
    }

    fn get_state(&self) -> ProcessableTorrentState {
        match self.state {
            QBittorrentTorrentState::Error => {
                ProcessableTorrentState::Other("Torrent Errored".into())
            }
            QBittorrentTorrentState::MissingFiles => {
                ProcessableTorrentState::Other("Torrent missing files".into())
            }
            QBittorrentTorrentState::Uploading => ProcessableTorrentState::Seeding,
            QBittorrentTorrentState::PausedUP => ProcessableTorrentState::Finished,
            QBittorrentTorrentState::QueuedUP => ProcessableTorrentState::Seeding,
            QBittorrentTorrentState::StalledUP => ProcessableTorrentState::Stalled,
            QBittorrentTorrentState::CheckingUP => {
                ProcessableTorrentState::Other("Checking torrent for upload".into())
            }
            QBittorrentTorrentState::ForcedUp => ProcessableTorrentState::Seeding,
            QBittorrentTorrentState::Allocating => {
                ProcessableTorrentState::Other("Allocating space for torrent".into())
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
                ProcessableTorrentState::Other("Checking torrent for download".into())
            }
            QBittorrentTorrentState::ForcedDL => {
                ProcessableTorrentState::Downloading(Some(self.progress))
            }
            QBittorrentTorrentState::CheckingResumeData => {
                ProcessableTorrentState::Other("Checking torrent to resume".into())
            }
            QBittorrentTorrentState::Moving => {
                ProcessableTorrentState::Other("Moving torrent".into())
            }
            QBittorrentTorrentState::Unknown => {
                ProcessableTorrentState::Other("Unknown issue occurred".into())
            }
        }
    }

    fn get_size_in_bytes(&self) -> Option<i64> {
        Some(self.size)
    }
}

impl TorrentContentInfo for QBittorrentTorrentContents {
    type FileIdType = u64;

    fn get_id(&self) -> Self::FileIdType {
        self.index
    }

    fn get_file_type(&self) -> &str {
        match self.name.rsplit_once(".") {
            Some((_, file_type)) => file_type,
            None => "UNKNOWN",
        }
    }
}
