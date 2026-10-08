// Copyright 2026 The Matrix.org Foundation C.I.C.
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

use matrix_sdk_base::deserialized_responses::ThreadSummary;
use ruma::{OwnedEventId, OwnedRoomId, events::receipt::ReceiptEventContent};
use tokio::sync::broadcast::{Receiver, Sender};

use super::super::{super::RoomEventCacheGenericUpdate, TimelineVectorDiffs};

/// An update related to events happened in a thread.
#[derive(Debug, Clone)]
pub enum ThreadEventCacheUpdate {
    /// The thread has received updates for the timeline as _diffs_.
    UpdateTimelineEvents(TimelineVectorDiffs),

    /// The thread summary has been updated.
    ///
    /// One can either observe [`ThreadInfo`] with
    /// [`ThreadEventCache::subscribe_to_thread_info`], or —if one is already
    /// listening to these updates— one can use this particular update to see
    /// new thread summary.
    ///
    /// [`ThreadInfo`]: matrix_sdk_base::event_cache::thread::ThreadInfo
    /// [`ThreadEventCache::subscribe_to_thread_info`]: super::ThreadEventCache::subscribe_to_thread_info
    UpdateSummary(ThreadSummary),

    /// The thread has received a new read receipt event.
    AddReadReceiptEvent {
        /// The event containing the receipts.
        event: ReceiptEventContent,
    },
}

/// Represents a [`ThreadInfo`]-ish update of a thread.
///
/// This is used by [`EventCache::subscribe_to_thread_info_generic_updates`][0].
/// Please read it to learn more about the motivation behind this type.
///
/// [0]: super::super::super::EventCache::subscribe_to_thread_info_generic_updates
#[derive(Clone, Debug)]
pub struct ThreadInfoGenericUpdate {
    /// The room ID owning the timeline.
    pub room_id: OwnedRoomId,

    /// The thread I being updated.
    pub thread_id: OwnedEventId,

    // The following are copied from `ThreadInfo`. Why? To reduce the size of this struct. Also, if
    // new fields are added in the future to `ThreadInfo`, it won't increase the size of this type.
    // Finally, some fields in `ThreadInfo`, like `read_receipts` contains data that should not be
    // shared, like `ReadReceipts::latest_active` or `ReadReceipts::pending`.

    //
    /// Copied from [`ThreadInfo::number_of_replies`][0].
    ///
    /// [0]: matrix_sdk_base::event_cache::thread::ThreadInfo::number_of_replies
    pub number_of_replies: u32,

    /// Copied from [`ThreadInfo::latest_event`][0].
    ///
    /// [0]: matrix_sdk_base::event_cache::thread::ThreadInfo::latest_event
    pub latest_event: Option<OwnedEventId>,

    /// Copied from [`ThreadInfo::read_receipts::num_unread`][0].
    ///
    /// [0]: matrix_sdk_base::read_receipts::ReadReceipts::num_unread
    pub num_unread: u64,

    /// Copied from [`ThreadInfo::read_receipts::num_notifications`][0].
    ///
    /// [0]: matrix_sdk_base::read_receipts::ReadReceipts::num_notifications
    pub num_notifications: u64,

    /// Copied from [`ThreadInfo::read_receipts::num_mentions`][0].
    ///
    /// [0]: matrix_sdk_base::read_receipts::ReadReceipts::num_mentions
    pub num_mentions: u64,
}

/// A small type to send updates in all channels.
#[derive(Clone)]
pub struct ThreadEventCacheUpdateSender {
    thread_sender: Sender<ThreadEventCacheUpdate>,
    room_generic_sender: Sender<RoomEventCacheGenericUpdate>,
}

impl ThreadEventCacheUpdateSender {
    /// Create a new [`ThreadEventCacheUpdateSender`].
    pub fn new(room_generic_sender: Sender<RoomEventCacheGenericUpdate>) -> Self {
        Self { thread_sender: Sender::new(32), room_generic_sender }
    }

    /// Send a [`TimelineVectorDiffs`].
    pub fn send(
        &self,
        thread_update: ThreadEventCacheUpdate,
        room_generic_update: Option<RoomEventCacheGenericUpdate>,
    ) {
        let _ = self.thread_sender.send(thread_update);

        if let Some(room_generic_update) = room_generic_update {
            let _ = self.room_generic_sender.send(room_generic_update);
        }
    }

    /// Create a new [`Receiver`] of [`ThreadEventCacheUpdate`].
    pub(super) fn new_thread_receiver(&self) -> Receiver<ThreadEventCacheUpdate> {
        self.thread_sender.subscribe()
    }
}
