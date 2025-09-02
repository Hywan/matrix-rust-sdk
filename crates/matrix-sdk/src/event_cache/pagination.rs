// Copyright 2024 The Matrix.org Foundation C.I.C.
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

//! A sub-object for running pagination tasks on a given room.

use std::{sync::Arc, time::Duration};

use eyeball::{SharedObservable, Subscriber};
use matrix_sdk_base::timeout::timeout;
use tracing::{debug, instrument, trace};

use super::{
    room::{LoadMoreEventsBackwardsOutcome, RoomEventCacheInner},
    BackPaginationOutcome, EventsOrigin, Result, RoomEventCacheUpdate,
};
use crate::event_cache::{EventCacheError, RoomEventCacheGenericUpdate};

/// Status for the back-pagination on a room event cache.
#[derive(Debug, PartialEq, Clone, Copy)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum RoomPaginationStatus {
    /// No back-pagination is happening right now.
    Idle {
        /// Have we hit the start of the timeline, i.e. back-paginating wouldn't
        /// have any effect?
        hit_timeline_start: bool,
    },

    /// Back-pagination is already running in the background.
    Paginating,
}

/// Small RAII guard to reset the pagination status on drop, if not disarmed in
/// the meanwhile.
struct ResetStatusOnDrop {
    prev_status: Option<RoomPaginationStatus>,
    pagination_status: SharedObservable<RoomPaginationStatus>,
}

impl ResetStatusOnDrop {
    /// Make the RAII guard have no effect.
    fn disarm(mut self) {
        self.prev_status = None;
    }
}

impl Drop for ResetStatusOnDrop {
    fn drop(&mut self) {
        if let Some(status) = self.prev_status.take() {
            let _ = self.pagination_status.set(status);
        }
    }
}

/// An API object to run pagination queries on a [`super::RoomEventCache`].
///
/// Can be created with [`super::RoomEventCache::pagination()`].
#[allow(missing_debug_implementations)]
#[derive(Clone)]
pub struct RoomPagination {
    pub(super) inner: Arc<RoomEventCacheInner>,
}

impl RoomPagination {
    /// Starts a back-pagination for the requested number of events.
    ///
    /// This automatically takes care of waiting for a pagination token from
    /// sync, if we haven't done that before.
    ///
    /// It will run multiple back-paginations until one of these two conditions
    /// is met:
    /// - either we've reached the start of the timeline,
    /// - or we've obtained enough events to fulfill the requested number of
    ///   events.
    #[instrument(skip(self))]
    #[doc(hidden)] // Only for tests. TODO: rewrite the tests to not use this method
    pub async fn run_backwards_until(
        &self,
        num_requested_events: u16,
    ) -> Result<BackPaginationOutcome> {
        let mut all_events = Vec::with_capacity(num_requested_events.into());

        loop {
            if let Some(outcome) = self.run_backwards_impl().await? {
                match outcome {
                    BackPaginationOutcome::Events { reached_start, mut events } => {
                        all_events.append(&mut events);

                        if reached_start || events.len() >= num_requested_events as usize {
                            return Ok(BackPaginationOutcome::Events {
                                reached_start,
                                events: all_events,
                            });
                        }
                    }

                    BackPaginationOutcome::Gap { reached_start, .. } => {
                        if reached_start {
                            return Ok(BackPaginationOutcome::Events {
                                reached_start,
                                events: all_events,
                            });
                        }
                    }
                }

                trace!(
                    "restarting back-pagination, because we haven't reached \
                     the start or obtained enough events yet"
                );
            }

            debug!("restarting back-pagination because of a timeline reset.");
        }
    }

    /// Run a single back-pagination.
    ///
    /// This automatically takes care of waiting for a pagination token from
    /// sync, if we haven't done that before.
    #[instrument(skip(self))]
    pub async fn run_backwards_once(&self) -> Result<BackPaginationOutcome> {
        eprintln!("=========== RUN BACKWARDS ONCE");

        loop {
            if let Some(outcome) = self.run_backwards_impl().await? {
                return Ok(outcome);
            }
            debug!("restarting back-pagination because of a timeline reset.");
        }
    }

    /// Paginate from the storage, and let pagination status observers know
    /// about updates.
    async fn run_backwards_impl(&self) -> Result<Option<BackPaginationOutcome>> {
        // First off, ensure there's no other ongoing back-pagination.
        let status_observable = &self.inner.pagination_status;
        let prev_status = status_observable.set(RoomPaginationStatus::Paginating);

        if !matches!(prev_status, RoomPaginationStatus::Idle { .. }) {
            return Err(EventCacheError::AlreadyBackpaginating);
        }

        let reset_status_on_drop_guard = ResetStatusOnDrop {
            prev_status: Some(prev_status),
            pagination_status: status_observable.clone(),
        };

        match self.paginate_backwards_impl().await? {
            Some(outcome) => {
                // Back-pagination's over and successful, don't reset the status to the previous
                // value.
                reset_status_on_drop_guard.disarm();

                // Notify subscribers that pagination ended.
                status_observable.set(RoomPaginationStatus::Idle {
                    hit_timeline_start: outcome.reached_start(),
                });

                Ok(Some(outcome))
            }

            None => {
                // We keep the previous status value, because we haven't obtained more
                // information about the pagination.
                Ok(None)
            }
        }
    }

    /// Paginate from the storage.
    ///
    /// This method isn't concerned with setting the pagination status; only the
    /// caller is.
    async fn paginate_backwards_impl(&self) -> Result<Option<BackPaginationOutcome>> {
        // A linked chunk might not be entirely loaded (if it's been lazy-loaded). Try
        // to load from storage first. We load nothing from the network: gaps are
        // resolved manually.

        loop {
            eprintln!("=== PAGINATE BACKWARDS IMPL ITERATION");

            let mut state_guard = self.inner.state.write().await;

            match state_guard.load_more_events_backwards().await? {
                LoadMoreEventsBackwardsOutcome::WaitForInitialPrevToken => {
                    const DEFAULT_WAIT_FOR_TOKEN_DURATION: Duration = Duration::from_secs(3);

                    // Release the state guard while waiting, to not deadlock the sync task.
                    drop(state_guard);

                    // Otherwise, wait for a notification that we received a previous-batch token.
                    trace!("waiting for a pagination token…");
                    let _ = timeout(
                        self.inner.pagination_batch_token_notifier.notified(),
                        DEFAULT_WAIT_FOR_TOKEN_DURATION,
                    )
                    .await;
                    trace!("done waiting");

                    self.inner.state.write().await.waited_for_initial_prev_token = true;

                    // Retry!
                    //
                    // Note: the next call to `load_more_events_backwards` can't return
                    // `WaitForInitialPrevToken` because we've just set to
                    // `waited_for_initial_prev_token`, so this is not an infinite loop.
                    //
                    // Note 2: not a recursive call, because recursive and async have a bad time
                    // together.
                    continue;
                }

                LoadMoreEventsBackwardsOutcome::Gap {
                    reached_on_disk_start: reached_start,
                    prev_token,
                } => {
                    let _ = self.inner.sender.send(RoomEventCacheUpdate::PrependTimelineGap {
                        prev_token: prev_token.clone(),
                        origin: EventsOrigin::Cache,
                    });

                    eprintln!("load more events backwards outcome => gap");

                    return Ok(Some(BackPaginationOutcome::Gap { reached_start, prev_token }));
                }

                LoadMoreEventsBackwardsOutcome::StartOfTimeline => {
                    eprintln!("load more events backwards outcome => start of timeline");

                    return Ok(Some(BackPaginationOutcome::Events {
                        reached_start: true,
                        events: vec![],
                    }));
                }

                LoadMoreEventsBackwardsOutcome::Events {
                    events,
                    timeline_event_diffs,
                    reached_on_disk_start: reached_start,
                } => {
                    eprintln!("load more events backwards outcome => events");

                    if !timeline_event_diffs.is_empty() {
                        let _ =
                            self.inner.sender.send(RoomEventCacheUpdate::UpdateTimelineEvents {
                                diffs: timeline_event_diffs,
                                origin: EventsOrigin::Cache,
                            });

                        // Send a room event cache generic update.
                        let _ =
                            self.inner.generic_update_sender.send(RoomEventCacheGenericUpdate {
                                room_id: self.inner.room_id.clone(),
                            });
                    }

                    return Ok(Some(BackPaginationOutcome::Events {
                        reached_start,
                        // This is a backwards pagination. `BackPaginationOutcome` expects events to
                        // be in “reverse order”.
                        events: events.into_iter().rev().collect(),
                    }));
                }
            }
        }
    }

    /// Returns a subscriber to the pagination status used for the
    /// back-pagination integrated to the event cache.
    pub fn status(&self) -> Subscriber<RoomPaginationStatus> {
        self.inner.pagination_status.subscribe()
    }
}
