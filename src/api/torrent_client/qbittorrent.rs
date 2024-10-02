use anyhow::format_err;
use async_trait::async_trait;
use qbittorrent::data::State;
use qbittorrent::queries::TorrentDownload;
use qbittorrent::traits::TorrentData;

use super::{Torrent, TorrentClient, TorrentFile, TorrentFilePriority, TorrentState};

pub struct QbittorrentWrapper {
    client: Option<qbittorrent::Api>,
}

impl QbittorrentWrapper {
    pub fn new() -> Self {
        Self { client: None }
    }
    fn convert_state(state: &State) -> TorrentState {
        match state {
            State::Error => TorrentState::Error(None),
            State::MissingFiles => TorrentState::Error(Some("MissingFiles".to_string())),
            State::Uploading => TorrentState::Uploading,
            State::PausedUP => TorrentState::Paused,
            State::QueuedUP => TorrentState::Paused, // Queued is essentially pausing until a slot is available
            State::StalledUP => TorrentState::StalledUpload,
            State::CheckingUP => TorrentState::Other("CheckingUP".to_string()),
            State::ForcedUP => TorrentState::Uploading,
            State::Allocating => TorrentState::Other("Allocating".to_string()),
            State::Downloading => TorrentState::Downloading,
            State::MetaDL => TorrentState::Starting,
            State::PausedDL => TorrentState::Paused,
            State::QueuedDL => TorrentState::Paused,
            State::StalledDL => TorrentState::Stalled,
            State::CheckingDL => TorrentState::Other("CheckingDL".to_string()),
            State::ForceDL => TorrentState::Downloading,
            State::CheckingResumeData => TorrentState::Other("CheckingResumeData".to_string()),
            State::Moving => TorrentState::Other("Moving".to_string()),
            State::Unknown => TorrentState::Other("Unknown".to_string()),
        }
    }
}

#[async_trait]
impl TorrentClient for QbittorrentWrapper {
    async fn initialise(
        &mut self,
        connection_uri: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        let username = username.unwrap_or_default();
        let password = password.unwrap_or_default();

        let client = qbittorrent::Api::new(username, password, connection_uri).await?;

        self.client = Some(client);
        Ok(())
    }

    async fn add_torrent(&self, magnet_uri: &str) -> Result<(), anyhow::Error> {
        let torrent = TorrentDownload::new(Some(magnet_uri.to_string()), None);
        let client = self.client.as_ref().unwrap();
        client.add_new_torrent(&torrent).await?;
        Ok(())
    }

    async fn add_torrents(&self, magnet_uris: Vec<&str>) -> Result<(), anyhow::Error> {
        for magnet in magnet_uris {
            self.add_torrent(magnet).await?;
        }
        Ok(())
    }

    async fn remove_torrent(&self, hash: &str) -> Result<(), anyhow::Error> {
        let hash = qbittorrent::data::Hash::from(hash.to_string());
        let client = self.client.as_ref().unwrap();
        client.delete_torrents(vec![&hash], false).await?;
        Ok(())
    }

    async fn remove_torrents(&self, hashes: Vec<&str>) -> Result<(), anyhow::Error> {
        let hashes = hashes
            .iter()
            .map(|hash| qbittorrent::data::Hash::from(hash.to_string()))
            .collect::<Vec<_>>();
        let hashes = hashes.iter().collect::<Vec<_>>();
        let client = self.client.as_ref().unwrap();
        client.delete_torrents(hashes, false).await?;
        Ok(())
    }

    async fn get_torrent(&self, hash: &str) -> Result<Torrent, anyhow::Error> {
        let client = self.client.as_ref().unwrap();
        let torrents = client.get_torrent_list().await?;
        let output = torrents
            .into_iter()
            .find(|torrent| torrent.hash().as_str() == hash);
        let output = match output {
            Some(torrent) => {
                let files = client.contents(&torrent).await?;
                let files = files
                    .into_iter()
                    .map(|file| {
                        let priority = match *file.priority() {
                            0 => TorrentFilePriority::DisallowDownload,
                            _ => TorrentFilePriority::AllowDownload,
                        };
                        TorrentFile::new(*file.index(), priority, file.name().clone())
                    })
                    .collect::<Vec<_>>();
                let files = if files.is_empty() { None } else { Some(files) };

                let state = Self::convert_state(torrent.state());
                Torrent::new(
                    None,
                    torrent.hash().to_string(),
                    *torrent.progress(),
                    state,
                    files,
                )
            }
            None => return Err(format_err!("Failed to find torrent")),
        };

        Ok(output)
    }

    async fn get_torrents(&self) -> Result<Vec<Torrent>, anyhow::Error> {
        let client = self.client.as_ref().unwrap();
        let torrents = client.get_torrent_list().await?;
        let mut output = Vec::new();
        for torrent in torrents {
            let files = client.contents(&torrent).await?;
            let files = files
                .into_iter()
                .map(|file| {
                    let priority = match *file.priority() {
                        0 => TorrentFilePriority::DisallowDownload,
                        _ => TorrentFilePriority::AllowDownload,
                    };
                    TorrentFile::new(*file.index(), priority, file.name().clone())
                })
                .collect::<Vec<_>>();
            let files = if files.is_empty() { None } else { Some(files) };

            let state = Self::convert_state(torrent.state());
            output.push(Torrent::new(
                None,
                torrent.hash().to_string(),
                *torrent.progress(),
                state,
                files,
            ));
        }

        Ok(output)
    }

    async fn set_file_priority(
        &self,
        hash: &str,
        file_ids: Vec<i64>,
        priority: TorrentFilePriority,
    ) -> Result<(), anyhow::Error> {
        let hash = qbittorrent::data::Hash::from(hash.to_string());
        let priority = match priority {
            TorrentFilePriority::AllowDownload => 1,
            TorrentFilePriority::DisallowDownload => 0,
        };
        let client = self.client.as_ref().unwrap();
        client.set_file_priority(&hash, file_ids, priority).await?;
        Ok(())
    }

    async fn reannounce_torrents(&self, hashes: Vec<&str>) -> Result<(), anyhow::Error> {
        let hashes = hashes
            .iter()
            .map(|hash| qbittorrent::data::Hash::from(hash.to_string()))
            .collect::<Vec<_>>();
        let hashes = hashes.iter().collect();
        let client = self.client.as_ref().unwrap();
        client.reannounce_torrents(hashes).await?;
        Ok(())
    }
}
