use crate::database::torrent::TorrentDBItem;
use crate::database::{Database, TorrentDB};
use crate::torrent::{TorrentClient, TorrentClientError, TorrentIdentifier};
use flume::{Receiver, Sender};
use log::{error, info};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;

pub struct TorrentManager<T: TorrentClient> {
    client: Arc<T>,
    database: Arc<Database>,
}

impl<T: TorrentClient + 'static> TorrentManager<T> {
    pub async fn connect(client: T, database: Arc<Database>) -> Result<Self, TorrentClientError> {
        match client.connect().await {
            Ok(connected) => {
                if connected {
                    Ok(Self {
                        client: Arc::new(client),
                        database,
                    })
                } else {
                    Err(TorrentClientError::ClientConnectError(
                        "Failed to connect to client".to_string(),
                    ))
                }
            }
            Err(err) => Err(err),
        }
    }

    pub fn start(self) -> (Sender<TorrentIdentifier>, JoinHandle<()>) {
        let (tx, rx) = flume::unbounded::<TorrentIdentifier>();

        let thread = tokio::spawn(async move {
            let manager = self;
            let rx = rx;
            let client = manager.client.clone();

            tokio::spawn(Self::check_for_new_torrents(client.clone(), rx)); // Add new torrents to client on signal
            tokio::spawn(Self::monitor_torrent_status(client, todo!()));
        });

        (tx, thread)
    }

    async fn check_for_new_torrents(client: Arc<T>, receiver: Receiver<TorrentIdentifier>) {
        while let Ok(identifier) = receiver.recv_async().await {
            match client.create_torrent(identifier).await {
                Ok(success) => {
                    if success {
                        info!("Added torrent to client!");
                    } else {
                        error!("Failed to add torrent to client");
                    }
                }
                Err(err) => {
                    error!("Error occurred adding torrent, {err}");
                }
            }
        }
    }

    async fn monitor_torrent_status(client: Arc<T>, database: Arc<Database>) {
        loop {
            tokio::time::sleep_until(tokio::time::Instant::now() + Duration::from_secs(15)).await;
            let torrents = match client.view_all_torrents().await {
                Ok(torrents) => torrents,
                Err(err) => {
                    error!("{err}");
                    continue;
                }
            };

            let torrent_db = TorrentDB::new(&database);
            let torrents: Vec<TorrentDBItem> =
                torrents.iter().map(|item| item.into()).collect();
            match torrent_db.upsert_torrents(torrents).await {
                Ok(_) => (),
                Err(err) => {
                    error!("Failed to upsert torrents: {err}");
                }
            }
        }
    }
}
