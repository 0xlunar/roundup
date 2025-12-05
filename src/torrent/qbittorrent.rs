use crate::torrent::{TorrentClient, TorrentClientError, TorrentIdentifier};
use wreq::multipart::Form;
use wreq::Client;

pub struct QBittorrent {
    client: Client,
    host: String,
    credentials: QBittorrentCredentials,
}

pub struct QBittorrentCredentials {
    username: String,
    password: String,
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

impl TorrentClient for QBittorrent {
    type TorrentType = ();

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
        identifier: TorrentIdentifier<'_>,
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

        if response.cookies().any(|cookie| cookie.name() == "SID") {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn update_torrent(
        &self,
        identifier: TorrentIdentifier<'_>,
    ) -> Result<bool, TorrentClientError> {
        todo!()
    }

    async fn stop_torrent(
        &self,
        identifier: TorrentIdentifier<'_>,
    ) -> Result<bool, TorrentClientError> {
        todo!()
    }

    async fn pause_torrent(
        &self,
        identifier: TorrentIdentifier<'_>,
    ) -> Result<bool, TorrentClientError> {
        todo!()
    }

    async fn resume_torrent(
        &self,
        identifier: TorrentIdentifier<'_>,
    ) -> Result<bool, TorrentClientError> {
        todo!()
    }

    async fn delete_torrent(
        &self,
        identifier: TorrentIdentifier<'_>,
        delete_file: bool,
    ) -> Result<bool, TorrentClientError> {
        todo!()
    }

    async fn reannounce_torrent(
        &self,
        identifier: TorrentIdentifier<'_>,
    ) -> Result<bool, TorrentClientError> {
        todo!()
    }

    async fn view_torrent(
        &self,
        identifier: TorrentIdentifier<'_>,
    ) -> Result<Self::TorrentType, TorrentClientError> {
        todo!()
    }

    async fn view_all_torrents(&self) -> Result<Vec<Self::TorrentType>, TorrentClientError> {
        todo!()
    }
}
