use std::sync::Arc;

use anyhow::format_err;
use async_trait::async_trait;
use rayon::prelude::*;
use tokio::sync::Mutex;
use transmission_rpc::TransClient;
use transmission_rpc::types::{
    BasicAuth, Id, TorrentAction, TorrentAddArgs, TorrentGetField, TorrentSetArgs, TorrentStatus,
};

use super::{Torrent, TorrentClient, TorrentFile, TorrentFilePriority, TorrentState};

pub struct TransmissionWrapper {
    client: Option<Arc<Mutex<TransClient>>>,
}

impl TransmissionWrapper {
    pub fn new() -> Self {
        Self { client: None }
    }
}

#[async_trait]
impl TorrentClient for TransmissionWrapper {
    async fn initialise(
        &mut self,
        connection_uri: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        let uri = url::Url::parse(connection_uri)?;

        let mut client = if username.is_some() && password.is_some() {
            let auth = BasicAuth {
                user: username.unwrap().to_string(),
                password: password.unwrap().to_string(),
            };
            TransClient::with_auth(uri, auth)
        } else {
            TransClient::new(uri)
        };

        // Test that transmission is active
        let session = client.session_get().await;
        match session {
            Ok(_) => {
                self.client = Some(Arc::new(Mutex::new(client)));
                Ok(())
            }
            Err(err) => Err(format_err!("{}", err)),
        }
    }

    async fn add_torrent(&self, magnet_uri: &str) -> Result<(), anyhow::Error> {
        let torrent_add_args = TorrentAddArgs {
            filename: Some(magnet_uri.to_string()),
            ..TorrentAddArgs::default()
        };
        let client = self.client.as_ref().unwrap();
        let mut client = client.lock().await;
        match client.torrent_add(torrent_add_args).await {
            Ok(_) => Ok(()),
            Err(err) => Err(format_err!("{}", err)),
        }
    }

    async fn add_torrents(&self, magnet_uris: Vec<&str>) -> Result<(), anyhow::Error> {
        let client = self.client.as_ref().unwrap();
        let mut client = client.lock().await;
        for magnet in magnet_uris {
            let torrent_add_args = TorrentAddArgs {
                filename: Some(magnet.to_string()),
                ..TorrentAddArgs::default()
            };
            match client.torrent_add(torrent_add_args).await {
                Ok(_) => (),
                Err(err) => return Err(format_err!("{}", err)),
            }
        }
        Ok(())
    }

    async fn remove_torrent(&self, hash: &str) -> Result<(), anyhow::Error> {
        let torrents = {
            let client = self.client.as_ref().unwrap();
            let mut client = client.lock().await;
            match client
                .torrent_get(
                    Some(vec![TorrentGetField::Id, TorrentGetField::HashString]),
                    None,
                )
                .await
            {
                Ok(t) => t,
                Err(err) => return Err(format_err!("{}", err)),
            }
        };

        let torrent = torrents
            .arguments
            .torrents
            .par_iter() // Incase the user has tonnes of torrents stored
            .find_any(|torrent| match &torrent.hash_string {
                Some(hash_string) => hash_string.as_str() == hash,
                None => false,
            });
        let id = match torrent {
            Some(t) => Id::Id(t.id.unwrap()),
            None => return Err(format_err!("Torrent does not exist")),
        };
        let client = self.client.as_ref().unwrap();
        let mut client = client.lock().await;
        match client.torrent_remove(vec![id], false).await {
            Ok(_) => Ok(()),
            Err(err) => Err(format_err!("{}", err)),
        }
    }

    async fn remove_torrents(&self, hashes: Vec<&str>) -> Result<(), anyhow::Error> {
        let client = self.client.as_ref().unwrap();
        let mut client = client.lock().await;
        let torrents = match client
            .torrent_get(
                Some(vec![TorrentGetField::Id, TorrentGetField::HashString]),
                None,
            )
            .await
        {
            Ok(t) => t,
            Err(err) => return Err(format_err!("{}", err)),
        };

        let torrents = torrents
            .arguments
            .torrents
            .par_iter() // Incase the user has tonnes of torrents stored
            .filter_map(|torrent| match &torrent.hash_string {
                Some(hash_string) if hashes.contains(&hash_string.as_str()) => {
                    Some(Id::Id(torrent.id.unwrap()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        match client.torrent_remove(torrents, false).await {
            Ok(_) => Ok(()),
            Err(err) => Err(format_err!("{}", err)),
        }
    }

    async fn get_torrent(&self, hash: &str) -> Result<Torrent, anyhow::Error> {
        let torrents = {
            let client = self.client.as_ref().unwrap();
            let mut client = client.lock().await;
            match client
                .torrent_get(
                    Some(vec![
                        TorrentGetField::Id,
                        TorrentGetField::HashString,
                        TorrentGetField::PercentDone,
                        TorrentGetField::Status,
                        TorrentGetField::Files,
                    ]),
                    None,
                )
                .await
            {
                Ok(t) => t,
                Err(err) => return Err(format_err!("{}", err)),
            }
        };

        let torrent = torrents
            .arguments
            .torrents
            .par_iter() // Incase the user has tonnes of torrents stored
            .find_any(|torrent| match &torrent.hash_string {
                Some(hash_string) => hash_string.as_str() == hash,
                None => false,
            });
        match torrent {
            Some(torrent) => {
                let id = torrent.id;
                let hash = torrent.hash_string.as_ref().unwrap().to_string();
                let percentage = *torrent.percent_done.as_ref().unwrap();
                let state = match torrent.status.as_ref().unwrap() {
                    TorrentStatus::Stopped => TorrentState::Paused,
                    TorrentStatus::QueuedToVerify => TorrentState::Paused,
                    TorrentStatus::Verifying => TorrentState::Other("Verify".to_string()),
                    TorrentStatus::QueuedToDownload => TorrentState::Paused,
                    TorrentStatus::Downloading => TorrentState::Downloading,
                    TorrentStatus::QueuedToSeed => TorrentState::Paused,
                    TorrentStatus::Seeding => TorrentState::Uploading,
                };
                let files = torrent.files.as_ref().unwrap();
                let files = files
                    .iter()
                    .enumerate()
                    .map(|(i, file)| {
                        TorrentFile::new(
                            i as i64,
                            TorrentFilePriority::AllowDownload,
                            file.name.clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                let files = if files.is_empty() { None } else { Some(files) };
                Ok(Torrent::new(id, hash, percentage as f64, state, files))
            }
            None => Err(format_err!("Unable to find torrent")),
        }
    }

    async fn get_torrents(&self) -> Result<Vec<Torrent>, anyhow::Error> {
        let torrents = {
            let client = self.client.as_ref().unwrap();
            let mut client = client.lock().await;
            match client
                .torrent_get(
                    Some(vec![
                        TorrentGetField::Id,
                        TorrentGetField::HashString,
                        TorrentGetField::PercentDone,
                        TorrentGetField::Status,
                        TorrentGetField::Files,
                    ]),
                    None,
                )
                .await
            {
                Ok(t) => t,
                Err(err) => return Err(format_err!("{}", err)),
            }
        };

        let torrents = torrents
            .arguments
            .torrents
            .par_iter()
            .map(|torrent| {
                let id = torrent.id;
                let hash = torrent.hash_string.as_ref().unwrap().to_string();
                let percentage = *torrent.percent_done.as_ref().unwrap();
                let state = match torrent.status.as_ref().unwrap() {
                    TorrentStatus::Stopped => TorrentState::Paused,
                    TorrentStatus::QueuedToVerify => TorrentState::Paused,
                    TorrentStatus::Verifying => TorrentState::Other("Verify".to_string()),
                    TorrentStatus::QueuedToDownload => TorrentState::Paused,
                    TorrentStatus::Downloading => TorrentState::Downloading,
                    TorrentStatus::QueuedToSeed => TorrentState::Paused,
                    TorrentStatus::Seeding => TorrentState::Uploading,
                };
                let files = torrent.files.as_ref().unwrap();
                let files = files
                    .iter()
                    .enumerate()
                    .map(|(i, file)| {
                        TorrentFile::new(
                            i as i64,
                            TorrentFilePriority::AllowDownload,
                            file.name.clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                let files = if files.is_empty() { None } else { Some(files) };
                Torrent::new(id, hash, percentage as f64, state, files)
            })
            .collect::<Vec<_>>();

        Ok(torrents)
    }

    async fn set_file_priority(
        &self,
        hash: &str,
        file_ids: Vec<i64>,
        priority: TorrentFilePriority,
    ) -> Result<(), anyhow::Error> {
        let torrent = self.get_torrent(hash).await?;

        let ids = file_ids
            .iter()
            .map(|file_id| *file_id as i32)
            .collect::<Vec<_>>();

        let torrent_set_args = match priority {
            TorrentFilePriority::AllowDownload => TorrentSetArgs {
                files_wanted: Some(ids),
                ..TorrentSetArgs::default()
            },
            TorrentFilePriority::DisallowDownload => TorrentSetArgs {
                files_unwanted: Some(ids),
                ..TorrentSetArgs::default()
            },
        };

        let id = match torrent.id {
            Some(id) => Id::Id(id),
            None => return Err(format_err!("Torrent does not exist")),
        };

        let client = self.client.as_ref().unwrap();
        let mut client = client.lock().await;
        match client.torrent_set(torrent_set_args, Some(vec![id])).await {
            Ok(_) => Ok(()),
            Err(err) => Err(format_err!("{}", err)),
        }
    }

    async fn reannounce_torrents(&self, hashes: Vec<&str>) -> Result<(), anyhow::Error> {
        let client = self.client.as_ref().unwrap();
        let mut client = client.lock().await;
        let torrents = match client
            .torrent_get(
                Some(vec![TorrentGetField::Id, TorrentGetField::HashString]),
                None,
            )
            .await
        {
            Ok(t) => t,
            Err(err) => return Err(format_err!("{}", err)),
        };

        let torrents = torrents
            .arguments
            .torrents
            .par_iter() // Incase the user has tonnes of torrents stored
            .filter_map(|torrent| match &torrent.hash_string {
                Some(hash_string) if hashes.contains(&hash_string.as_str()) => {
                    Some(Id::Id(torrent.id.unwrap()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        match client
            .torrent_action(TorrentAction::Reannounce, torrents)
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => Err(format_err!("{}", err)),
        }
    }
}
