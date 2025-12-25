use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::ops::Mul;
use std::sync::Arc;

pub mod qbittorrent;

#[async_trait]
pub trait TorrentClient: Send + Sync {
    type TorrentType<'de>: TorrentInfo + Deserialize<'de>
    where
        Self: 'de;
    type TorrentContentType<'de>: TorrentContentInfo + Deserialize<'de>
    where
        Self: 'de;

    async fn connect(&self) -> Result<bool, TorrentClientError>;
    async fn create_torrent(
        &self,
        identifier: TorrentIdentifier,
    ) -> Result<bool, TorrentClientError>;
    async fn update_file_priority(
        &self,
        identifier: TorrentIdentifier,
        files: Vec<<Self::TorrentContentType<'_> as TorrentContentInfo>::FileIdType>,
    ) -> Result<bool, TorrentClientError>;
    async fn pause_torrent(
        &self,
        identifier: TorrentIdentifier,
    ) -> Result<bool, TorrentClientError>;
    async fn resume_torrent(
        &self,
        identifier: TorrentIdentifier,
    ) -> Result<bool, TorrentClientError>;
    async fn delete_torrent(
        &self,
        identifier: TorrentIdentifier,
        delete_file: bool,
    ) -> Result<bool, TorrentClientError>;

    async fn reannounce_torrent(
        &self,
        identifier: TorrentIdentifier,
    ) -> Result<bool, TorrentClientError>;
    async fn view_torrent_contents(
        &self,
        identifier: TorrentIdentifier,
    ) -> Result<Vec<Self::TorrentContentType<'_>>, TorrentClientError>;
    async fn view_all_torrents(&self) -> Result<Vec<Self::TorrentType<'_>>, TorrentClientError>;
}

pub trait TorrentInfo: Send + Sync {
    fn get_id(&self) -> &str;
    fn as_identifier(&self) -> TorrentIdentifier;
    fn get_state(&self) -> ProcessableTorrentState;
    fn get_size_in_bytes(&self) -> Option<u64>;
}

pub trait TorrentContentInfo: Send + Sync {
    type FileIdType: Serialize + Clone + Send + Sync;
    fn get_id(&self) -> Self::FileIdType;
    fn get_file_type(&self) -> &str;
}

impl<'a, T: TorrentInfo> From<&'a T> for crate::database::torrent::TorrentDBItem<'a> {
    fn from(value: &'a T) -> Self {
        Self {
            id: value.get_id(),
            state: value.get_state(),
            bytes: value.get_size_in_bytes(),
        }
    }
}

#[derive(Debug)]
pub enum ProcessableTorrentState {
    Downloading(Option<f64>),
    Paused,
    Seeding,
    Finished,
    Stalled,
    Other(&'static str),
}

impl Display for ProcessableTorrentState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessableTorrentState::Downloading(state) => match state {
                Some(state) => {
                    f.write_fmt(format_args!("Downloading: {}%", state.mul(100.00).floor()))
                }
                None => f.write_str("Downloading"),
            },
            ProcessableTorrentState::Paused => f.write_str("Paused"),
            ProcessableTorrentState::Seeding => f.write_str("Seeding"),
            ProcessableTorrentState::Finished => f.write_str("Finished"),
            ProcessableTorrentState::Stalled => f.write_str("Stalled"),
            ProcessableTorrentState::Other(message) => {
                f.write_fmt(format_args!("Other: {}", message))
            }
        }
    }
}

pub enum TorrentIdentifier {
    Hash(Arc<str>),
    Magnet(Arc<str>),
}

#[derive(Debug)]
pub enum TorrentClientError {
    ClientConnectError(String),
    CreateTorrentError(String),
    PauseTorrentError(String),
    ResumeTorrentError(String),
    UpdateFilePriorityError(String),
    ReannounceTorrentError(String),
    DeleteTorrentError(String),
    ViewTorrentError(String),
    Anyhow(anyhow::Error),
}

impl core::error::Error for TorrentClientError {}

impl Display for TorrentClientError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TorrentClientError::ClientConnectError(msg) => {
                f.write_fmt(format_args!("ClientConnectError: {}", msg))
            }
            TorrentClientError::CreateTorrentError(msg) => {
                f.write_fmt(format_args!("CreateTorrentError: {}", msg))
            }
            TorrentClientError::PauseTorrentError(msg) => {
                f.write_fmt(format_args!("PauseTorrentError: {}", msg))
            }
            TorrentClientError::ResumeTorrentError(msg) => {
                f.write_fmt(format_args!("ResumeTorrentError: {}", msg))
            }
            TorrentClientError::UpdateFilePriorityError(msg) => {
                f.write_fmt(format_args!("UpdateTorrentError: {}", msg))
            }
            TorrentClientError::ReannounceTorrentError(msg) => {
                f.write_fmt(format_args!("ReannounceTorrentError: {}", msg))
            }
            TorrentClientError::DeleteTorrentError(msg) => {
                f.write_fmt(format_args!("DeleteTorrentError: {}", msg))
            }
            TorrentClientError::ViewTorrentError(msg) => {
                f.write_fmt(format_args!("ViewTorrentError: {}", msg))
            }
            TorrentClientError::Anyhow(err) => f.write_fmt(format_args!("AnyhowError: {}", err)),
        }
    }
}

impl<'a> TorrentIdentifier {
    pub fn new_hash(hash: &'a str) -> Self {
        Self::Hash(hash.into())
    }

    pub fn new_magnet(magnet: &'a str) -> Self {
        Self::Magnet(magnet.into())
    }

    pub fn to_hash(&self) -> Option<Arc<str>> {
        match self {
            TorrentIdentifier::Hash(hash) => Some(hash.clone()),
            TorrentIdentifier::Magnet(magnet) => match magnet.split_once("btih:") {
                Some((_, part)) => part.split_once("?").map(|(hash, _)| hash.into()),
                None => None,
            },
        }
    }
}
