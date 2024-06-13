// Copyright 2023 The Matrix.org Foundation C.I.C.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{collections::BTreeMap, time::Duration};

use futures_util::future::join_all;
use matrix_sdk_common::failures_cache::FailuresCache;
use ruma::{
    events::room::encrypted::{EncryptedEventScheme, OriginalSyncRoomEncryptedEvent},
    serde::Raw,
    OwnedEventId, OwnedRoomId,
};
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tracing::{trace, warn};

use crate::{
    client::WeakClient,
    encryption::backups::UploadState,
    executor::{spawn, JoinHandle},
    Client,
};

#[derive(Default)]
pub(crate) struct ClientTasks {
    #[cfg(feature = "e2e-encryption")]
    pub(crate) upload_room_keys: Option<BackupUploadingTask>,
    #[cfg(feature = "e2e-encryption")]
    pub(crate) download_room_keys: Option<BackupDownloadTask>,
    pub(crate) setup_e2ee: Option<JoinHandle<()>>,
}

#[cfg(feature = "e2e-encryption")]
pub(crate) struct BackupUploadingTask {
    sender: mpsc::UnboundedSender<()>,
    #[allow(dead_code)]
    join_handle: JoinHandle<()>,
}

#[cfg(feature = "e2e-encryption")]
impl Drop for BackupUploadingTask {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.join_handle.abort();
    }
}

#[cfg(feature = "e2e-encryption")]
impl BackupUploadingTask {
    pub(crate) fn new(client: WeakClient) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();

        let join_handle = spawn(async move {
            Self::listen(client, receiver).await;
        });

        Self { sender, join_handle }
    }

    pub(crate) fn trigger_upload(&self) {
        let _ = self.sender.send(());
    }

    pub(crate) async fn listen(client: WeakClient, mut receiver: UnboundedReceiver<()>) {
        while receiver.recv().await.is_some() {
            if let Some(client) = client.get() {
                let upload_progress = &client.inner.e2ee.backup_state.upload_progress;

                if let Err(e) = client.encryption().backups().backup_room_keys().await {
                    upload_progress.set(UploadState::Error);
                    warn!("Error backing up room keys {e:?}");
                    // Note: it's expected we're not `continue`ing here, because
                    // *every* single state update
                    // is propagated to the caller.
                }

                upload_progress.set(UploadState::Idle);
            } else {
                trace!("Client got dropped, shutting down the task");
                break;
            }
        }
    }
}

/// Information about a request for a backup download for an undecryptable
/// event.
#[derive(Debug)]
struct RoomKeyDownloadRequest {
    /// The room in which the event was sent.
    room_id: OwnedRoomId,

    /// The ID of the event we could not decrypt.
    event_id: OwnedEventId,

    /// The megolm session that the event was encrypted with.
    megolm_session_id: String,
}

impl RoomKeyDownloadRequest {
    pub fn to_room_key_info(&self) -> RoomKeyInfo {
        (self.room_id.clone(), self.megolm_session_id.clone())
    }
}

pub type RoomKeyInfo = (OwnedRoomId, String);
pub type TaskQueue = BTreeMap<RoomKeyInfo, JoinHandle<()>>;

pub(crate) struct BackupDownloadTask {
    sender: mpsc::UnboundedSender<RoomKeyDownloadRequest>,
    #[allow(dead_code)]
    join_handle: JoinHandle<()>,
}

#[cfg(feature = "e2e-encryption")]
impl Drop for BackupDownloadTask {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.join_handle.abort();
    }
}

impl BackupDownloadTask {
    const DOWNLOAD_DELAY_MILLIS: u64 = 100;

    pub(crate) fn new(client: WeakClient) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();

        let join_handle = spawn(async move {
            Self::listen(
                client,
                receiver,
                FailuresCache::with_settings(Duration::from_secs(60 * 60 * 24), 60),
            )
            .await;
        });

        Self { sender, join_handle }
    }

    /// Trigger a backup download for the keys for the given event.
    ///
    /// Does nothing unless the event is encrypted using `m.megolm.v1.aes-sha2`.
    /// Otherwise, tells the listener task to set off a task to do a backup
    /// download, unless there is one already running.
    pub(crate) fn trigger_download_for_utd_event(
        &self,
        room_id: OwnedRoomId,
        event: Raw<OriginalSyncRoomEncryptedEvent>,
    ) {
        if let Ok(deserialized_event) = event.deserialize() {
            if let EncryptedEventScheme::MegolmV1AesSha2(c) = deserialized_event.content.scheme {
                let _ = self.sender.send(RoomKeyDownloadRequest {
                    room_id,
                    event_id: deserialized_event.event_id,
                    megolm_session_id: c.session_id,
                });
            }
        }
    }

    pub(crate) async fn download(
        client: Client,
        room_key_info: RoomKeyInfo,
        failures_cache: FailuresCache<RoomKeyInfo>,
    ) {
        // Wait a bit, perhaps the room key will arrive in the meantime.
        tokio::time::sleep(Duration::from_millis(Self::DOWNLOAD_DELAY_MILLIS)).await;

        if let Some(machine) = client.olm_machine().await.as_ref() {
            let (room_id, session_id) = &room_key_info;

            if !machine.is_room_key_available(room_id, session_id).await.unwrap() {
                match client.encryption().backups().download_room_key(room_id, session_id).await {
                    Ok(_) => failures_cache.remove(std::iter::once(&room_key_info)),
                    Err(_) => failures_cache.insert(room_key_info),
                }
            }
        }
    }

    pub(crate) async fn prune_tasks(task_queue: &mut TaskQueue) {
        let mut handles = Vec::with_capacity(task_queue.len());

        while let Some((_, handle)) = task_queue.pop_first() {
            handles.push(handle);
        }

        join_all(handles).await;
    }

    pub(crate) async fn listen(
        client: WeakClient,
        mut receiver: UnboundedReceiver<RoomKeyDownloadRequest>,
        failures_cache: FailuresCache<RoomKeyInfo>,
    ) {
        let mut task_queue = TaskQueue::new();

        while let Some(room_key_download_request) = receiver.recv().await {
            let room_key_info = room_key_download_request.to_room_key_info();
            trace!(?room_key_info, "Got a request to download a room key from the backup");

            if task_queue.len() >= 10 {
                Self::prune_tasks(&mut task_queue).await
            }

            if let Some(client) = client.get() {
                let backups = client.encryption().backups();

                let already_tried = failures_cache.contains(&room_key_info);
                let task_exists = task_queue.contains_key(&room_key_info);

                if !already_tried && !task_exists && backups.are_enabled().await {
                    let task = spawn(Self::download(
                        client,
                        room_key_info.to_owned(),
                        failures_cache.to_owned(),
                    ));

                    task_queue.insert(room_key_info, task);
                }
            } else {
                trace!("Client got dropped, shutting down the task");
                break;
            }
        }
    }
}
