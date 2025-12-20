use crate::database::Database;
use crate::torrent::{ProcessableTorrentState, TorrentInfo};

pub struct TorrentDB<'a> {
    database: &'a Database,
}

impl<'a> TorrentDB<'a> {
    pub fn new(database: &'a Database) -> Self {
        Self { database }
    }

    pub async fn upsert_torrents(&self, data: Vec<TorrentDBItem<'_>>) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct TorrentDBItem<'a> {
    pub id: &'a str,
    pub state: ProcessableTorrentState,
    pub bytes: Option<u64>,
}
