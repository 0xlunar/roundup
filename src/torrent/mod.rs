mod qbittorrent;

pub trait TorrentClient {
    type TorrentType;
    async fn connect(&self) -> Result<bool, TorrentClientError>;
    async fn create_torrent(
        &self,
        identifier: TorrentIdentifier,
    ) -> Result<bool, TorrentClientError>;
    async fn update_torrent(
        &self,
        identifier: TorrentIdentifier,
    ) -> Result<bool, TorrentClientError>;
    async fn stop_torrent(&self, identifier: TorrentIdentifier)
    -> Result<bool, TorrentClientError>;
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
    async fn view_torrent(
        &self,
        identifier: TorrentIdentifier,
    ) -> Result<Self::TorrentType, TorrentClientError>;
    async fn view_all_torrents(&self) -> Result<Vec<Self::TorrentType>, TorrentClientError>;
}

pub enum TorrentIdentifier<'a> {
    Hash(&'a str),
    Magnet(&'a str),
}

#[non_exhaustive]
pub enum TorrentClientError {
    ClientConnectError(String),
    CreateTorrentError(String),
    StopTorrentError(String),
    PauseTorrentError(String),
    ResumeTorrentError(String),
    ReannounceTorrentError(String),
    DeleteTorrentError(String),
    ViewTorrentError(String),
    Anyhow(anyhow::Error),
}
