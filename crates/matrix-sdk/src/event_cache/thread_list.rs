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

//! List all threads owned by a room, known by the `EventCache` and by the
//! homeserver.

use std::{fmt, sync::Arc};

use eyeball_im::{ObservableVector, VectorSubscriberBatchedStream};
use imbl::Vector;
use matrix_sdk_base::event_cache::thread::ThreadInfo;
use ruma::OwnedEventId;

use super::EventCacheInner;

/// Represents a list of threads for a particular room.
pub struct ThreadList {
    event_cache_inner: Arc<EventCacheInner>,
    threads: ObservableVector<Arc<ThreadListItem>>,
}

impl fmt::Debug for ThreadList {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ThreadList").field("threads", &self.threads).finish_non_exhaustive()
    }
}

impl ThreadList {
    pub(super) fn new(
        event_cache_inner: Arc<EventCacheInner>,
        all_threads: Vec<(OwnedEventId, ThreadInfo)>,
    ) -> Self {
        let mut threads = ObservableVector::new();
        threads.append(
            all_threads
                .into_iter()
                .map(|(thread_root, thread_info)| {
                    Arc::new(ThreadListItem { thread_root, thread_info })
                })
                .collect(),
        );

        Self { event_cache_inner, threads }
    }

    /// Subscribe to the updates of this [`ThreadList`].
    pub fn subscribe(
        &self,
    ) -> (Vector<Arc<ThreadListItem>>, VectorSubscriberBatchedStream<Arc<ThreadListItem>>) {
        self.threads.subscribe().into_values_and_batched_stream()
    }
}

/// Represents an item (an entry) in a [`ThreadList`].
#[derive(Clone, Debug)]
pub struct ThreadListItem {
    /// The unthreaded event that is the root of the thread.
    pub thread_root: OwnedEventId,

    /// Information about the thread.
    pub thread_info: ThreadInfo,
}

#[cfg(test)]
mod tests {
    use matrix_sdk_test::{JoinedRoomBuilder, async_test, event_factory::EventFactory};
    use ruma::{event_id, room_id, user_id};
    use stream_assert::assert_pending;

    use crate::test_utils::mocks::MatrixMockServer;

    #[async_test]
    async fn test_subscribe_to_empty_initial_items() {
        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;

        let room_id = room_id!("!r0");

        let event_factory =
            EventFactory::new().sender(user_id!("@mnt_io:matrix.org")).room(room_id);

        let event_cache = client.event_cache();

        server
            .sync_room(
                &client,
                JoinedRoomBuilder::new(room_id)
                    .add_timeline_bulk([event_factory.text_msg("foo").into_raw_sync()]),
            )
            .await;

        let thread_list = event_cache.thread_list(room_id).await.unwrap();
        let (initial_items, mut stream) = thread_list.subscribe();

        assert!(initial_items.is_empty());
        assert_pending!(stream);
    }

    #[async_test]
    async fn test_subscribe_to_non_empty_initial_items() {
        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;

        let room_id = room_id!("!r0");
        let thread_0_id = event_id!("$t0");
        let thread_0_latest_event = event_id!("$ev0");
        let thread_1_id = event_id!("$t1");
        let thread_1_latest_event = event_id!("$ev1");

        let event_factory =
            EventFactory::new().sender(user_id!("@mnt_io:matrix.org")).room(room_id);

        let event_cache = client.event_cache();

        server
            .sync_room(
                &client,
                JoinedRoomBuilder::new(room_id).add_timeline_bulk([
                    event_factory.text_msg("thread root #0").event_id(thread_0_id).into_raw_sync(),
                    event_factory.text_msg("thread root #1").event_id(thread_1_id).into_raw_sync(),
                    event_factory
                        .text_msg("event #0_0")
                        .event_id(thread_0_latest_event)
                        .in_thread(thread_0_id, thread_0_id)
                        .into_raw_sync(),
                    event_factory
                        .text_msg("event #1_0")
                        .event_id(thread_1_latest_event)
                        .in_thread(thread_1_id, thread_1_id)
                        .into_raw_sync(),
                ]),
            )
            .await;

        let thread_list = event_cache.thread_list(room_id).await.unwrap();
        let (initial_items, mut stream) = thread_list.subscribe();

        assert_eq!(initial_items.len(), 2);

        assert_eq!(initial_items[0].thread_root, thread_1_id);
        assert_eq!(initial_items[0].thread_info.number_of_replies, 1);
        assert_eq!(
            initial_items[0].thread_info.latest_event,
            Some(thread_1_latest_event.to_owned())
        );

        assert_eq!(initial_items[1].thread_root, thread_0_id);
        assert_eq!(initial_items[1].thread_info.number_of_replies, 1);
        assert_eq!(
            initial_items[1].thread_info.latest_event,
            Some(thread_0_latest_event.to_owned())
        );

        assert_pending!(stream);
    }
}
