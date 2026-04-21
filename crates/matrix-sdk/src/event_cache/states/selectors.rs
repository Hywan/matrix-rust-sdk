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

//! This module contains the [`CacheState`] trait to select a specific cache
//! state in the full [`State`].

#![allow(private_interfaces)]

use ruma::{OwnedEventId, OwnedRoomId};

use super::{
    super::EventCacheError, PinnedEventCacheState, RoomEventCacheState, State,
    ThreadEventCacheState,
};

/// Trait to select a specific state of a cache inside a [`State`].
pub trait CacheState {
    /// The type of the specific state of cache.
    type Item;

    /// Immutably select a specific state of a cache inside a [`State`].
    fn select<'state>(&self, state: &'state State) -> Option<&'state Self::Item>;

    /// Mutably select a specific state of a cache inside a [`State`].
    fn select_mut<'state>(&self, state: &'state mut State) -> Option<&'state mut Self::Item>;
}

/// Select a [`RoomEventCacheState`] in [`State`].
pub struct RoomEventCacheStateSelector(OwnedRoomId);

impl CacheState for RoomEventCacheStateSelector {
    type Item = RoomEventCacheState;

    fn select<'state>(&self, state: &'state State) -> Option<&'state Self::Item> {
        state.rooms.get(&self.0)
    }

    fn select_mut<'state>(&self, state: &'state mut State) -> Option<&'state mut Self::Item> {
        state.rooms.get_mut(&self.0)
    }
}

impl From<&RoomEventCacheStateSelector> for EventCacheError {
    fn from(value: &RoomEventCacheStateSelector) -> Self {
        Self::RoomNotFound { room_id: value.0.clone() }
    }
}

/// Select a [`ThreadEventCacheState`] in [`State`].
pub struct ThreadEventCacheStateSelector(OwnedRoomId, OwnedEventId);

impl CacheState for ThreadEventCacheStateSelector {
    type Item = ThreadEventCacheState;

    fn select<'state>(&self, state: &'state State) -> Option<&'state Self::Item> {
        state.threads.get(&self.0).and_then(|threads_for_room| threads_for_room.get(&self.1))
    }

    fn select_mut<'state>(&self, state: &'state mut State) -> Option<&'state mut Self::Item> {
        state
            .threads
            .get_mut(&self.0)
            .and_then(|threads_for_room| threads_for_room.get_mut(&self.1))
    }
}

impl From<&ThreadEventCacheStateSelector> for EventCacheError {
    fn from(value: &ThreadEventCacheStateSelector) -> Self {
        Self::ThreadNotFound { room_id: value.0.clone(), thread_id: value.1.clone() }
    }
}

/// Select a [`PinnedEventCacheState`] in [`State`].
pub struct PinnedEventCacheStateSelector(OwnedRoomId);

impl CacheState for PinnedEventCacheStateSelector {
    type Item = PinnedEventCacheState;

    fn select<'state>(&self, state: &'state State) -> Option<&'state Self::Item> {
        state.pinned_events.get(&self.0)
    }

    fn select_mut<'state>(&self, state: &'state mut State) -> Option<&'state mut Self::Item> {
        state.pinned_events.get_mut(&self.0)
    }
}

impl From<&PinnedEventCacheStateSelector> for EventCacheError {
    fn from(value: &PinnedEventCacheStateSelector) -> Self {
        Self::PinnedEventsNotFound { room_id: value.0.clone() }
    }
}
