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

use std::sync::Arc;

use eyeball_im::{ObservableVector, VectorSubscriberBatchedStream};
use imbl::Vector;
use matrix_sdk_base::event_cache::thread::ThreadInfo;
use ruma::OwnedEventId;

/// Represents a list of threads for a particular room.
#[derive(Debug)]
pub struct ThreadList {
    threads: ObservableVector<Arc<ThreadListItem>>,
}

impl ThreadList {
    pub(super) fn new(all_threads: Vec<(OwnedEventId, ThreadInfo)>) -> Self {
        let mut threads = ObservableVector::new();
        threads.append(
            all_threads
                .into_iter()
                .map(|(thread_root, thread_info)| {
                    Arc::new(ThreadListItem { thread_root, thread_info })
                })
                .collect(),
        );

        Self { threads }
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
    use matrix_sdk_base::read_receipts::ReadReceipts;
    use ruma::event_id;
    use stream_assert::assert_pending;

    use super::{ThreadInfo, ThreadList};

    #[test]
    fn test_subscribe_to_empty_initial_items() {
        let thread_list = ThreadList::new(vec![]);
        let (initial_items, mut stream) = thread_list.subscribe();

        assert!(initial_items.is_empty());
        assert_pending!(stream);
    }

    #[test]
    fn test_subscribe_to_non_empty_initial_items() {
        let thread_0_id = event_id!("$t0");
        let thread_0_latest_event = event_id!("$ev0");
        let thread_1_id = event_id!("$t1");
        let thread_1_latest_event = event_id!("$ev1");

        let thread_list = ThreadList::new(vec![
            (
                thread_0_id.to_owned(),
                ThreadInfo {
                    number_of_replies: 1,
                    latest_event: Some(thread_0_latest_event.to_owned()),
                    read_receipts: ReadReceipts::default(),
                },
            ),
            (
                thread_1_id.to_owned(),
                ThreadInfo {
                    number_of_replies: 2,
                    latest_event: Some(thread_1_latest_event.to_owned()),
                    read_receipts: ReadReceipts::default(),
                },
            ),
        ]);
        let (initial_items, mut stream) = thread_list.subscribe();

        assert_eq!(initial_items.len(), 2);

        assert_eq!(initial_items[0].thread_root, thread_0_id);
        assert_eq!(initial_items[0].thread_info.number_of_replies, 1);
        assert_eq!(
            initial_items[0].thread_info.latest_event,
            Some(thread_0_latest_event.to_owned())
        );

        assert_eq!(initial_items[1].thread_root, thread_1_id);
        assert_eq!(initial_items[1].thread_info.number_of_replies, 2);
        assert_eq!(
            initial_items[1].thread_info.latest_event,
            Some(thread_1_latest_event.to_owned())
        );

        assert_pending!(stream);
    }
}
