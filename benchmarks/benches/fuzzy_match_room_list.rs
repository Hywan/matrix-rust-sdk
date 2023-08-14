use criterion::*;
use futures_util::{pin_mut, StreamExt};
use matrix_sdk::{
    config::RequestConfig,
    matrix_auth::{Session, SessionTokens},
    Client,
};
use matrix_sdk_base::SessionMeta;
use matrix_sdk_ui::room_list_service::{
    filters::new_filter_fuzzy_match_room_name, Error, RoomListService,
    ALL_ROOMS_LIST_NAME as ALL_ROOMS,
};
use ruma::{api::MatrixVersion, device_id, user_id};
use tokio::runtime::Builder;
use wiremock::{http::Method, Match, MockServer, Request};

fn criterion() -> Criterion {
    #[cfg(target_os = "linux")]
    let criterion = Criterion::default().with_profiler(pprof::criterion::PProfProfiler::new(
        100,
        pprof::criterion::Output::Flamegraph(None),
    ));

    #[cfg(not(target_os = "linux"))]
    let criterion = Criterion::default();

    criterion
}

async fn new_client() -> (Client, MockServer) {
    let session = Session {
        meta: SessionMeta {
            user_id: user_id!("@example:localhost").to_owned(),
            device_id: device_id!("DEVICEID").to_owned(),
        },
        tokens: SessionTokens { access_token: "1234".to_owned(), refresh_token: None },
    };

    let server = MockServer::start().await;
    let client = Client::builder()
        .homeserver_url(server.uri())
        .server_versions([MatrixVersion::V1_0])
        .request_config(RequestConfig::new().disable_retry())
        .build()
        .await
        .unwrap();
    client.restore_session(session).await.unwrap();

    (client, server)
}

async fn new_room_list() -> Result<(Client, MockServer, RoomListService), Error> {
    let (client, server) = new_client().await;

    Ok((client.clone(), server, RoomListService::new_with_encryption(client).await?))
}

struct SlidingSyncMatcher;

#[derive(serde::Deserialize)]
struct PartialSlidingSyncRequest {
    pub txn_id: Option<String>,
}

impl Match for SlidingSyncMatcher {
    fn matches(&self, request: &Request) -> bool {
        request.url.path() == "/_matrix/client/unstable/org.matrix.msc3575/sync"
            && request.method == Method::Post
    }
}

/// Run a single sliding sync request, checking that the request is a subset of
/// what we expect it to be, and providing the given next response.
#[macro_export]
macro_rules! sync_then_fake_response {
    (
        [$server:ident, $stream:ident]
        respond with = $response_json:ident
        $(,)?
    ) => {{
        use assert_matches::assert_matches;
        use wiremock::{Mock, Request, ResponseTemplate};
        use $crate::{PartialSlidingSyncRequest, SlidingSyncMatcher};

        let _mock_guard = Mock::given(SlidingSyncMatcher)
            .respond_with(move |request: &Request| {
                let partial_request: PartialSlidingSyncRequest = request.body_json().unwrap();
                let mut json = $response_json.clone();
                json.as_object_mut()
                    .unwrap()
                    .insert("txn_id".to_owned(), partial_request.txn_id.into());

                ResponseTemplate::new(200).set_body_json(json)
                // $response_json
                // json!({
                //     "txn_id": partial_request.txn_id,
                //     $( $response_json )*
                // })
                // )
            })
            .mount_as_scoped(&$server)
            .await;

        let next = $stream.next().await;

        assert_matches!(next, Some(Ok(_)), "sync's result");

        next
    }};
}

pub fn fuzzy_match_room_list(c: &mut Criterion) {
    let runtime = Builder::new_multi_thread().enable_all().build().expect("Can't create runtime");

    // Prelude.
    let mut response = serde_json::json!({
        "pos": "1",
        "lists": {
            ALL_ROOMS: {
                "count": 10,
                "ops": [
                    {
                        "op": "SYNC",
                        "range": [1, 4],
                        "room_ids": [
                            "!r1:bar.org",
                            "!r2:bar.org",
                            "!r3:bar.org",
                            "!r4:bar.org",
                        ],
                    },
                ],
            },
        },
        "rooms": {
            "!r1:bar.org": {
                "name": "Matrix Bar",
                "initial": true,
                "timeline": [],
            },
            "!r2:bar.org": {
                "name": "Hello",
                "initial": true,
                "timeline": [],
            },
            "!r3:bar.org": {
                "name": "Helios live",
                "initial": true,
                "timeline": [],
            },
            "!r4:bar.org": {
                "name": "Matrix Baz",
                "initial": true,
                "timeline": [],
            },
        },
    });

    {
        let rooms =
            response.as_object_mut().unwrap().get_mut("rooms").unwrap().as_object_mut().unwrap();

        for i in 5..4000 {
            rooms.insert(
                format!("!r{i}:bar.org"),
                serde_json::json!({"name": format!("Matrix Heb({i})la"), "initial": true, "timeline": []}),
            );
        }
    }

    // Start the benchmark.

    let mut group = c.benchmark_group("Group");
    // group.throughput(Throughput::Elements(100));

    const NAME: &str = "NAME";

    // Bench.

    group.bench_with_input(BenchmarkId::new("function name", NAME), &response, |b, response| {
        b.to_async(&runtime).iter(|| async {
            let (client, server, room_list) = new_room_list().await.unwrap();

            let sync = room_list.sync();
            pin_mut!(sync);

            let all_rooms = room_list.all_rooms().await.unwrap();

            let (entries_stream_dynamic_filter, dynamic_filter) =
                all_rooms.entries_with_dynamic_filter();
            pin_mut!(entries_stream_dynamic_filter);

            dynamic_filter.set(new_filter_fuzzy_match_room_name(&client, "mat ba"));

            let response = response.clone();

            sync_then_fake_response! {
                [server, sync]
                respond with = response,
            };

            let _a = entries_stream_dynamic_filter.next().await;

            dynamic_filter.set(new_filter_fuzzy_match_room_name(&client, "M"));
            let _a = entries_stream_dynamic_filter.next().await;

            dynamic_filter.set(new_filter_fuzzy_match_room_name(&client, "Ma"));
            let _a = entries_stream_dynamic_filter.next().await;

            dynamic_filter.set(new_filter_fuzzy_match_room_name(&client, "Mat"));
            let _a = entries_stream_dynamic_filter.next().await;
        })
    });

    group.finish()
}

criterion_group! {
    name = benches;
    config = criterion();
    targets = fuzzy_match_room_list
}
criterion_main!(benches);
