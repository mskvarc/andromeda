//! Clause 4.9 `RegExp`, end to end.
//!
//! Clause 4.9: "`RegExp` shall be a regular expression as mandated by IEEE 1003.2 [11]", and Table 5.2.8-1 says the same of `idPattern`. IEEE 1003.2 is POSIX.2, which the `regex` crate (in memory) and PostgreSQL's `~` operator (in the store) implement alike in the large and differ on in the small.
//!
//! A pattern validated in one dialect and executed in the other would surface PostgreSQL's refusal as `500 InternalError`, though a malformed pattern is client input and owes a `400`. A pattern both engines compile but read differently would answer one request from PostgreSQL and the next from the `regex` crate. A stored query is answered by the store and a subscription's `q` in memory, so both entry points are exercised.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, problem_detail as error};
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

/// The entity type every fixture in this module carries.
const FIXTURE_TYPE: &str = "Building";

/// The Property the pattern operators match against.
const MATCHED_ATTRIBUTE: &str = "name";

/// How long the first notifications are waited for.
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a further notification is waited for once every expected one has
/// arrived, so a subscription that must not fire has had its chance to.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(3);

/// How many deliveries are read before giving up on the ones under test.
const DELIVERIES_READ: usize = 12;

async fn create(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED, "create entity: {:?}", response.body);
}

async fn query(client: &TestClient, query_str: &str) -> (StatusCode, Value) {
    let response = client
        .query_entities(query_str, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    let body = response.json_value().expect("query response must be valid JSON");
    (response.status, body)
}

fn ids_from_response(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("response must be a JSON array")
        .iter()
        .map(|entity| entity["id"].as_str().expect("entity must have an id member").to_owned())
        .collect()
}

/// Store one entity whose matched Property holds `value`, and return its id.
async fn create_named(scope: &TestEnvironment, client: &TestClient, suffix: &str, value: &str) -> String {
    let id = scope.id(&format!("{FIXTURE_TYPE}:{suffix}"));
    create(
        client,
        &json!({
            "id": id,
            "type": FIXTURE_TYPE,
            MATCHED_ATTRIBUTE: {"type": "Property", "value": value},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    id
}

/// A `q` pattern filter over one test's own entities, percent-encoded for the
/// HTTP binding (clause 4.9).
///
/// The candidate set is pinned to `ids` because the suite runs its tests
/// concurrently against one database: an unrestricted filter would match a
/// sibling test's fixtures and assert on their ordering rather than on the
/// pattern under test.
fn pattern_filter(ids: &[&str], pattern: &str) -> String {
    format!(
        "type={FIXTURE_TYPE}&local=true&id={}&q={}",
        id_filter(ids),
        urlencoding::encode(&format!("{MATCHED_ATTRIBUTE}~=\"{pattern}\""))
    )
}

/// The comma-separated `id` list selecting exactly these entities.
fn id_filter(ids: &[&str]) -> String {
    ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(",")
}

/// The `q` a subscription carries to put `pattern` through the in-memory
/// evaluator rather than the store.
fn subscription_filter(pattern: &str) -> String {
    format!("{MATCHED_ATTRIBUTE}~=\"{pattern}\"")
}

/// Create one subscription over the fixture type whose `q` is `filter`, and
/// return the response so the caller can assert on its status.
async fn create_pattern_subscription(scope: &TestEnvironment, receiver: &NotificationServer, local_id: &str, filter: &str) -> (StatusCode, Value) {
    let subscription = json!({
        "id": scope.id(local_id),
        "type": "Subscription",
        "entities": [{"type": FIXTURE_TYPE, "idPattern": format!("^{}:", scope.id_prefix())}],
        "notificationTrigger": ["entityCreated", "attributeUpdated"],
        "q": filter,
        "notification": {
            "endpoint": {
                "uri": receiver.endpoint().expect("notification endpoint must be valid"),
                "accept": "application/json"
            }
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = scope.client().create_subscription(&subscription).await.expect("create subscription failed");
    let body = created.json_value().unwrap_or(Value::Null);
    (created.status, body)
}

/// Every (subscription id, notified entity id) pair the receiver saw, read
/// until `expected` notifications have arrived and then once more so a
/// subscription that must not fire has had its chance to.
async fn delivered_pairs(receiver: &NotificationServer, expected: usize) -> BTreeSet<(String, String)> {
    let mut delivered = BTreeSet::new();
    for _ in 0..DELIVERIES_READ {
        let timeout = if delivered.len() < expected { DELIVERY_TIMEOUT } else { SETTLE_TIMEOUT };
        let Ok(captured) = receiver.next_request(timeout).await else {
            break;
        };
        let body: Value = captured.json().expect("a notification body is JSON");
        let subscription = body["subscriptionId"].as_str().expect("a notification names its subscription").to_owned();
        for entity in body["data"].as_array().into_iter().flatten() {
            let id = entity["id"].as_str().expect("a notified entity has an id").to_owned();
            delivered.insert((subscription.clone(), id));
        }
    }
    delivered
}

/// A Unicode property class compiles in the `regex` crate but PostgreSQL answers it with "invalid escape \\ sequence"; executing it inside SQL must not surface as `500 InternalError`.
#[tokio::test]
async fn regression_query_pattern_a_construct_postgresql_cannot_run_is_a_bad_request_not_an_internal_error() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    // Entities have to exist: an empty candidate set never reaches the pattern.
    let id = create_named(&scope, client, "dialect-a", "Mercedes").await;

    let query_str = format!(
        "type={FIXTURE_TYPE}&local=true&id={}&idPattern={}",
        id_filter(&[&id]),
        urlencoding::encode(r"\p{L}")
    );
    let (status, body) = query(client, &query_str).await;
    assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR, "a malformed pattern is never a broker fault: {body}");
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["type"], error::BAD_REQUEST_DATA.as_str(), "{body}");

    scope.finish().await.expect("cleanup failed");
}

/// Each of these was executed against PostgreSQL and refused with "invalid
/// escape \\ sequence", "invalid embedded option", "invalid character class",
/// "invalid character range" or "quantifier operand invalid". Reaching SQL with
/// any of them turns a malformed request into a database error.
#[tokio::test]
async fn regression_query_pattern_every_construct_only_one_engine_supports_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "dialect-b", "Mercedes").await;

    for pattern in [
        r"\p{L}",
        r"\P{L}",
        r"a\z",
        r"\x{61}",
        "(?m:ab)",
        "(?u)a",
        "(?U)a+",
        "(?-s)a",
        r"Merc\x33de",
        "[[:^alpha:]]",
        "[a-b-c]",
        "[[:alpha:]-z]",
    ] {
        let (status, body) = query(client, &pattern_filter(&[&id], pattern)).await;
        assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR, "{pattern} must not be a broker fault: {body}");
        assert_eq!(status, StatusCode::BAD_REQUEST, "{pattern}: {body}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// `\b` compiles in both engines and means two different things — a word
/// boundary to the `regex` crate, a backspace character to PostgreSQL — so one
/// filter selected different entities depending on which engine answered it.
#[tokio::test]
async fn regression_query_pattern_an_assertion_the_two_engines_read_differently_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "dialect-c", "Mercedes").await;

    for pattern in [r"\bMer", r"Mer\Bced"] {
        let (status, body) = query(client, &pattern_filter(&[&id], pattern)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{pattern} means two things: {body}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// The `regex` crate has no parse path for POSIX.2's equivalence classes or collating symbols, so it reads `[[=a=]]` as a nested class over `{'=','a'}` where PostgreSQL reads the equivalence class of `a`. Refusing these narrows what clause 4.9 mandates but keeps one filter from giving two answers. The same parse and refusal cover the `regex` crate's own nested-class extension and a mistyped POSIX class name, which PostgreSQL rejects outright.
#[tokio::test]
async fn regression_query_pattern_a_bracket_expression_the_two_engines_read_differently_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "dialect-bracket", "Mercedes").await;

    for pattern in ["[[=a=]]", "^[[=a=]]$", "[[.a.]]", "[[abc]]", "[[:alpha]]", "[a-z&&b-d]", "[]-a]", "[--/]"] {
        let (status, body) = query(client, &pattern_filter(&[&id], pattern)).await;
        assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR, "{pattern} must not be a broker fault: {body}");
        assert_eq!(status, StatusCode::BAD_REQUEST, "{pattern} means two things: {body}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// A subscription's `q` is the request that reaches the in-memory evaluator, so
/// a construct the two engines read differently has to be refused there too.
/// Accepting it at this entry point would make one filter mean two things: the
/// store would answer the query and the `regex` crate the subscription.
#[tokio::test]
async fn regression_query_pattern_the_subscription_entry_point_refuses_the_same_constructs() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");

    for (index, pattern) in ["^[[=a=]]$", "[[:^alpha:]]", "(?m:Merc)", r"\bMer", r"\p{L}"].iter().enumerate() {
        let filter = subscription_filter(pattern);
        let local_id = format!("Subscription:pattern-refused-{index}");
        let (status, body) = create_pattern_subscription(&scope, &receiver, &local_id, &filter).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{pattern} reaches the in-memory evaluator: {body}");
        assert_eq!(body["type"], error::BAD_REQUEST_DATA.as_str(), "{pattern}: {body}");
    }

    receiver.shutdown().await.expect("notification server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// POSIX.2 gives `.` no newline exception and PostgreSQL's default is newline-insensitive matching, so `Merc.des` selects a value spelled `Merc\ndes`; the `regex` crate's default is the opposite, so the in-memory engine is built to POSIX's reading. The control subscription cannot match the fixture at all, so a broker that delivered every change rather than the matching one fails this too.
#[tokio::test]
async fn regression_query_pattern_a_dot_selects_a_newline_in_both_engines() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");

    let matching = scope.id("Subscription:dot-newline-matching");
    let control = scope.id("Subscription:dot-newline-control");
    for (local_id, pattern) in [("Subscription:dot-newline-matching", "Merc.des"), ("Subscription:dot-newline-control", "^Xerc")] {
        let (status, body) = create_pattern_subscription(&scope, &receiver, local_id, &subscription_filter(pattern)).await;
        assert_eq!(status, StatusCode::CREATED, "{pattern} is a POSIX.2 extended regular expression: {body}");
    }

    let id = create_named(&scope, client, "dot-newline", "Merc\ndes").await;

    let delivered = delivered_pairs(&receiver, 1).await;
    assert!(
        delivered.contains(&(matching.clone(), id.clone())),
        "the in-memory evaluator reads `.` as POSIX does, so it selects a newline: {delivered:?}"
    );
    assert!(
        !delivered.iter().any(|(subscription, _)| subscription == &control),
        "the control filter matches nothing the fixture holds: {delivered:?}"
    );

    let (status, body) = query(client, &pattern_filter(&[&id], "Merc.des")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![id], "the store reads `.` the same way");

    receiver.shutdown().await.expect("notification server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9 mandates IEEE 1003.2, so every POSIX.2 extended regular
/// expression has to be accepted and executed. This is the positive control for
/// the refusals above: PostgreSQL accepts each of these and selects the stored
/// value, and so does the broker.
#[tokio::test]
async fn regression_query_pattern_every_posix_extended_regular_expression_is_executed() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "posix-match", "Mercedes").await;

    for pattern in [
        "^Merc",
        "M.*s$",
        "Merc[e]des",
        "Mer(c|k)edes",
        "e.{2}e",
        "[[:alpha:]]+",
        "[[:lower:]]+",
        "[]M]erc",
        "Merc{1}edes",
        "Mer[a-z]+es",
        // A hyphen that opens or closes its list is a literal to both engines,
        // and an escaped one is a literal wherever it stands.
        "[-M]erc",
        "Merc[e-]des",
        "[M\\-]erc",
        // Clause 4.9 delegates `RegExp` to IEEE 1003.2 and only `quotedStr` to RFC 8259, so `\.`, `\(`, `\)` and `\+` must reach the pattern engine as written although none is a JSON escape.
        r"Merc\.?edes",
        r"^\(?Merc",
        r"Merc\+?edes",
        r"Mercedes\$?",
    ] {
        let (status, body) = query(client, &pattern_filter(&[&id], pattern)).await;
        assert_eq!(status, StatusCode::OK, "{pattern} is a POSIX.2 extended regular expression: {body}");
        assert_eq!(ids_from_response(&body), vec![id.clone()], "{pattern} must match the stored value");
    }

    scope.finish().await.expect("cleanup failed");
}

/// The extensions the two engines share stay available. The shorthand classes are among them: the store runs their Unicode ranges rather than PostgreSQL's own shorthand, which classifies only ASCII under `COLLATE "C"`.
#[tokio::test]
async fn regression_query_pattern_a_shorthand_both_engines_share_is_still_executed() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "shared-shorthand", "Merc3des").await;

    for pattern in [
        "(?m)^Merc",
        "(?s)M.*s",
        "(?x)M e r c",
        "(?:Merc)+",
        "[[:alnum:]]+",
        "[Merc[:digit:]]+",
        "Merc[[:digit:]]de",
        r"Merc\dde",
        r"^\w+$",
        r"^\S+$",
        r"Merc[\d]\D",
    ] {
        let (status, body) = query(client, &pattern_filter(&[&id], pattern)).await;
        assert_eq!(status, StatusCode::OK, "{pattern} is common to both engines: {body}");
        assert_eq!(ids_from_response(&body), vec![id.clone()], "{pattern} must match the stored value");
    }

    scope.finish().await.expect("cleanup failed");
}

/// PostgreSQL answers a repetition applied to another repetition with "quantifier operand invalid", so the broker refuses it before it reaches SQL.
#[tokio::test]
async fn regression_query_pattern_a_repeated_quantifier_is_still_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "repeated-quantifier", "Mercedes").await;

    let (status, body) = query(client, &pattern_filter(&[&id], "Merc**")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "PostgreSQL rejects a repetition operand: {body}");

    scope.finish().await.expect("cleanup failed");
}
