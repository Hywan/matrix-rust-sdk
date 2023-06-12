use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use matrix_sdk::RoomListEntry;

pub fn new_filter_by_room_name(
    pattern: String,
) -> impl Fn(&RoomListEntry) -> bool + Send + Sync + 'static {
    let searcher = SkimMatcherV2::default().ignore_case().smart_case().use_cache(true);

    move |room_list_entry| -> bool { searcher.fuzzy_match("room_name", &pattern).is_some() }
}
