use crate::database::Database;
use crate::torrent::ProcessableTorrentState;

pub struct TorrentDB<'a> {
    database: &'a Database,
}

impl<'a> TorrentDB<'a> {
    pub fn new(database: &'a Database) -> Self {
        Self { database }
    }

    pub async fn upsert_torrents(&self, data: Vec<TorrentDBItem<'_>>) -> anyhow::Result<()> {
        todo!("Upsert not implemented yet")
    }

    pub async fn delete_torrents(&self, data: Vec<TorrentDBItem<'_>>) -> anyhow::Result<()> {
        todo!("delete not implemented yet")
    }

    pub async fn get_excluded_file_types(&self) -> anyhow::Result<Vec<String>> {
        todo!("get excluded file types not implemented yet")
    }
}

pub struct TorrentDBItem<'a> {
    pub id: &'a str,
    pub state: ProcessableTorrentState,
    pub bytes: Option<u64>,
}
