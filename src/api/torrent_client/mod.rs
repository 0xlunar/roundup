use std::fmt::{Display, Formatter, Write};

use async_trait::async_trait;

pub mod qbittorrent;
pub mod transmission;

#[async_trait]
pub trait TorrentClient: Send + Sync {
    async fn initialise(
        &mut self,
        connection_uri: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<(), anyhow::Error>;
    async fn add_torrent(&self, magnet_uri: &str) -> Result<(), anyhow::Error>;
    async fn add_torrents(&self, magnet_uris: Vec<&str>) -> Result<(), anyhow::Error>;
    async fn remove_torrent(&self, hash: &str) -> Result<(), anyhow::Error>;
    async fn remove_torrents(&self, hashes: Vec<&str>) -> Result<(), anyhow::Error>;
    async fn get_torrent(&self, hash: &str) -> Result<Torrent, anyhow::Error>;
    async fn get_torrents(&self) -> Result<Vec<Torrent>, anyhow::Error>;
    async fn set_file_priority(
        &self,
        hash: &str,
        file_ids: Vec<i64>,
        priority: TorrentFilePriority,
    ) -> Result<(), anyhow::Error>;
    async fn reannounce_torrents(&self, hashes: Vec<&str>) -> Result<(), anyhow::Error>;
}

#[derive(Default)]
pub struct Torrent {
    pub id: Option<i64>,
    pub hash: String,
    pub progress: f64,
    pub state: TorrentState,
    pub files: Option<Vec<TorrentFile>>,
}

#[derive(Default)]
pub struct TorrentFile {
    pub id: i64,
    pub priority: TorrentFilePriority,
    pub file_name: String,
}
#[derive(Default)]
pub enum TorrentFilePriority {
    #[default]
    AllowDownload,
    DisallowDownload,
}

#[derive(Default, PartialEq, Eq, Clone)]
pub enum TorrentState {
    #[default]
    Starting,
    Downloading,
    Stalled,
    Paused,
    Completed,
    Uploading,
    StalledUpload,
    Error(Option<String>),
    Other(String),
}

impl Torrent {
    pub fn new(
        id: Option<i64>,
        hash: String,
        progress: f64,
        state: TorrentState,
        files: Option<Vec<TorrentFile>>,
    ) -> Self {
        Self {
            id,
            hash,
            progress,
            state,
            files,
        }
    }
}

impl TorrentFile {
    pub fn new(id: i64, priority: TorrentFilePriority, file_name: String) -> Self {
        Self {
            id,
            priority,
            file_name,
        }
    }
}

impl Display for TorrentState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TorrentState::Starting => f.write_str("Starting"),
            TorrentState::Downloading => f.write_str("Downloading"),
            TorrentState::Stalled => f.write_str("Stalled"),
            TorrentState::Paused => f.write_str("Paused"),
            TorrentState::Completed => f.write_str("Completed"),
            TorrentState::Uploading => f.write_str("Uploading"),
            TorrentState::StalledUpload => f.write_str("StalledUpload"),
            TorrentState::Error(err) => match err {
                Some(err) => {
                    let text = format!("Error: {}", err);
                    f.write_str(&text)
                }
                None => f.write_str("Error"),
            },
            TorrentState::Other(other) => f.write_str(other),
        }
    }
}
