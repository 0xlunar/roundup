use crate::database::{Database, TorrentDB};
use crate::torrent::{
    ProcessableTorrentState, TorrentClient, TorrentClientError, TorrentContentInfo,
    TorrentIdentifier, TorrentInfo,
};
use anyhow::format_err;
use flume::{Receiver, Sender};
use log::{error, info, warn};
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

pub struct TorrentManager<T: TorrentClient> {
    client: Arc<T>,
    database: Arc<Database>,
}

struct ExcludeFileTypes {
    interior: Arc<RwLock<ExcludeTorrentItemsInterior>>,
    database: Arc<Database>,
    expiry: Duration,
}

struct ExcludeTorrentItemsInterior {
    items: Option<Arc<Vec<String>>>,
    last_updated: Instant,
}

impl ExcludeFileTypes {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            interior: Arc::new(RwLock::new(ExcludeTorrentItemsInterior {
                items: None,
                last_updated: Instant::now(),
            })),
            database,
            expiry: Duration::from_hours(12),
        }
    }

    pub async fn get(&self) -> Option<Arc<Vec<String>>> {
        {
            let read_lock = self.interior.read().await;

            if read_lock.items.is_none()
                || read_lock
                    .last_updated
                    .duration_since(Instant::now())
                    .ge(&self.expiry)
            {
                match self.update().await {
                    Ok(_) => (),
                    Err(err) => {
                        error!("{err}");
                        return None;
                    }
                };
            }
        }

        let read_lock = self.interior.read().await;
        read_lock.items.as_ref().map(|items| items.clone())
    }

    async fn update(&self) -> anyhow::Result<()> {
        let torrent_db = TorrentDB::new(&self.database);
        let items = match torrent_db.get_excluded_file_types().await {
            Ok(items) => items,
            Err(err) => return Err(format_err!("Failed to get excluded file types: {err}")),
        };

        {
            let mut write_lock = self.interior.write().await;
            write_lock.last_updated = Instant::now();
            write_lock.items = Some(Arc::new(items));
        }
        Ok(())
    }
}

impl<'de, T: TorrentClient + 'static> TorrentManager<T> {
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
            tokio::spawn(Self::monitor_torrent_status(client, manager.database));
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
        let exclude_file_types = ExcludeFileTypes::new(database.clone());
        let reannounce_duration = Duration::from_mins(15);
        let loop_duration = Duration::from_secs(15);
        let mut reannounce_timer = Instant::now();
        loop {
            tokio::time::sleep_until(tokio::time::Instant::now() + loop_duration).await;
            // Insert or update torrents in database
            let all_torrents = match client.view_all_torrents().await {
                Ok(torrents) => torrents,
                Err(err) => {
                    error!("{err}");
                    continue;
                }
            };

            Self::upsert_all_torrents(database.clone(), &all_torrents).await;

            // Remove Completed torrents that aren't seeding
            Self::remove_completed_torrents(client.clone(), database.clone(), &all_torrents).await;

            // Prevent downloading unwanted items (any item not a video or subtitles) (IGNORE MANUALLY ADDED TORRENTS)
            let excluded_file_types = exclude_file_types.get().await;
            Self::remove_unwanted_file_types(client.clone(), &all_torrents, excluded_file_types)
                .await;

            // Check for stalled torrents and reannounce but on a longer timer
            if reannounce_timer.elapsed() >= reannounce_duration {
                Self::reannounce_stalled_torrents(client.clone(), &all_torrents).await;
                reannounce_timer = Instant::now();
            }
        }
    }

    async fn upsert_all_torrents(database: Arc<Database>, torrents: &[T::TorrentType<'de>]) {
        let torrent_db = TorrentDB::new(&database);
        let torrents: Vec<_> = torrents.iter().map(|item| item.into()).collect();
        match torrent_db.update_torrent_state(torrents).await {
            Ok(_) => (),
            Err(err) => {
                error!("Failed to upsert torrents: {err}");
            }
        }
    }

    async fn remove_completed_torrents(
        client: Arc<T>,
        database: Arc<Database>,
        torrents: &[T::TorrentType<'de>],
    ) {
        let completed = torrents
            .iter()
            .filter(|torrent| matches!(torrent.get_state(), ProcessableTorrentState::Finished))
            .collect::<Vec<_>>();
        let completed_db_torrents = completed
            .iter()
            .map(|completed| completed.get_id())
            .collect::<Vec<_>>();

        let torrent_db = TorrentDB::new(&database);
        match torrent_db.delete_torrents(&completed_db_torrents).await {
            Ok(_) => (),
            Err(err) => {
                error!("Error deleting torrents from database: {err}");
                return;
            }
        }
        for torrent in completed {
            match client.delete_torrent(torrent.as_identifier(), false).await {
                Ok(success) => {
                    if !success {
                        error!("Unsuccessful deleting torrent");
                    }
                }
                Err(err) => {
                    error!("{err}");
                }
            }
        }
    }

    async fn reannounce_stalled_torrents(client: Arc<T>, torrents: &[T::TorrentType<'de>]) {
        let stalled_torrents = torrents
            .iter()
            .filter(|torrent| matches!(torrent.get_state(), ProcessableTorrentState::Stalled))
            .collect::<Vec<_>>();

        for torrent in stalled_torrents {
            match client.reannounce_torrent(torrent.as_identifier()).await {
                Ok(success) => {
                    if !success {
                        warn!("Unable to reannounce torrent");
                    }
                }
                Err(err) => {
                    error!("{err}");
                }
            }
        }
    }

    async fn remove_unwanted_file_types(
        client: Arc<T>,
        torrents: &[T::TorrentType<'de>],
        exclude_torrent_items: Option<Arc<Vec<String>>>,
    ) {
        if let Some(exclude_torrent_items) = exclude_torrent_items {
            let downloading_torrents = torrents
                .iter()
                .filter(|torrent| {
                    matches!(torrent.get_state(), ProcessableTorrentState::Downloading(_))
                })
                .collect::<Vec<_>>();

            for torrent in downloading_torrents {
                let contents = match client.view_torrent_contents(torrent.as_identifier()).await {
                    Ok(contents) => contents,
                    Err(err) => {
                        error!("{err}");
                        continue;
                    }
                };

                let items = contents
                    .into_iter()
                    .filter_map(|item| {
                        let file_type = item.get_file_type();
                        if exclude_torrent_items
                            .iter()
                            .any(|exclude| file_type == *exclude)
                        {
                            Some(item.get_id())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();

                match client
                    .update_file_priority(torrent.as_identifier(), items)
                    .await
                {
                    Ok(success) => {
                        if !success {
                            warn!("Failed to update files ");
                        }
                    }
                    Err(err) => {
                        error!("{err}");
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
enum TorrentManagerError {
    UpsertFailure(String),
    TorrentClientError(TorrentClientError),
    Anyhow(anyhow::Error),
}

impl Display for TorrentManagerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TorrentManagerError::UpsertFailure(msg) => {
                f.write_fmt(format_args!("upsert failure: {msg}"))
            }
            TorrentManagerError::TorrentClientError(err) => {
                f.write_fmt(format_args!("torrent client error: {err}"))
            }
            TorrentManagerError::Anyhow(err) => f.write_fmt(format_args!("error: {err}")),
        }
    }
}

impl core::error::Error for TorrentManagerError {}
