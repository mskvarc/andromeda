//! A pooled database pool killed under load is answered 503 with
//! `Retry-After`, never 500, on every replica and on every path.
//!
//! Clause 5.5.6 makes a failure the client did not cause an `InternalError`;
//! clause 6.3.2 admits the HTTP binding's own 503, which RFC 7231 section
//! 6.6.4 defines for a condition that clears after the delay `Retry-After`
//! names. While `PgBouncer`'s pool for a tenant database is killed, the pooler
//! drops the broker's connections and fails new logins, which the broker
//! must read as the store being unreachable. Every batch write starts by
//! reading the tenant's registrations, so that lookup is what meets the
//! outage first; it is this broker's own store, never a context source.

use crate::{client, context_source_adapter::ContextSourceAdapter, control_pool, link_header, reset, tenant_database};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::pooler_probe::{endpoints, pooler_console::PoolerConsole, tenant_load};
use axum::http::Method;
use reqwest::Client;
use serde_json::{Value, json};
use std::{collections::BTreeMap, slice, time::Duration};
use tokio::time::{Instant, sleep};

/// How long each test may drive requests through the outage before the
/// tenant must serve again. `PgBouncer` caches the failed server login of a
/// killed pool for `server_login_retry` (15 s by default), and each request
/// meanwhile waits out the broker's acquire deadline.
const RECOVERY_DEADLINE: Duration = Duration::from_secs(45);

/// How long the pool stays killed before it is resumed.
const KILLED_FOR: Duration = Duration::from_secs(1);

/// The pause between two requests of the loop.
const REQUEST_SPACING: Duration = Duration::from_millis(200);

/// One answer: its status and its `Retry-After`, or `None` when the request
/// got no answer at all.
type Answer = Option<(u16, Option<String>)>;

/// Kill the pooler's pool for `database`, and resume it after [`KILLED_FOR`].
async fn kill_then_resume(database: &str) {
    let console = PoolerConsole::new(endpoints::pgbouncer_admin_url());
    let _ = console.kill(database).await;
    sleep(KILLED_FOR).await;
    let _ = console.resume_database(database).await;
}

/// The status and `Retry-After` of one response.
fn answer_of(response: Result<reqwest::Response, reqwest::Error>) -> Answer {
    response.ok().map(|response| {
        let retry_after = response.headers().get("Retry-After").and_then(|value| value.to_str().ok()).map(str::to_owned);
        (response.status().as_u16(), retry_after)
    })
}

/// Upsert the one entity `id` under `tenant` through `broker`.
async fn upsert(client: &Client, broker: &str, tenant: &str, id: &str) -> Answer {
    answer_of(
        client
            .post(format!("{broker}entityOperations/upsert"))
            .header("NGSILD-Tenant", tenant)
            .header("Content-Type", "application/json")
            .header("Link", link_header())
            .json(&json!([{ "id": id, "type": "Device" }]))
            .send()
            .await,
    )
}

/// Whether the request loop may stop: the tenant served a request after its
/// pool was resumed, or the recovery deadline passed.
fn outage_is_over(started: Instant, served: bool) -> bool {
    (served && started.elapsed() > KILLED_FOR * 2) || started.elapsed() > RECOVERY_DEADLINE
}

/// Assert no answer of the outage is a 500, and every 503 says when to retry:
/// after one second for a pool that could not hand out a connection in time,
/// after five for a store that could not be reached.
fn assert_retryable_or_landed(answers: &[Answer], landed: &[u16]) {
    for answer in answers {
        match answer {
            Some((503, retry_after)) => assert!(matches!(retry_after.as_deref(), Some("1" | "5")), "a 503 must say when to retry: {answers:?}"),
            Some((status, _)) => assert!(landed.contains(status), "an outage is answered 503, never {status}: {answers:?}"),
            None => {}
        }
    }
}

/// Clause 5.5.6 with RFC 7231 6.6.4: while a tenant's pool is killed every
/// batch upsert is answered 201 or a 503 with `Retry-After`, never 500, and
/// the tenant serves again once the pool is resumed.
#[tokio::test]
#[serial_test::serial]
async fn a_killed_pool_answers_batch_upserts_503_never_500() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let tenant = tenant_load::tenant_names("pgbkillup", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1)
        .await
        .expect("every tenant is provisioned");
    let database = tenant_database(&pool).await;

    let drive = async {
        let mut answers = Vec::new();
        let started = Instant::now();
        let mut sequence = 0_u32;
        loop {
            sequence += 1;
            let answer = upsert(&client, &broker, &tenant, &format!("urn:ngsi-ld:Device:outage:{sequence}")).await;
            let served = matches!(answer, Some((201, _)));
            answers.push(answer);
            if outage_is_over(started, served) {
                return answers;
            }
            sleep(REQUEST_SPACING).await;
        }
    };
    let (answers, ()) = tokio::join!(drive, kill_then_resume(&database));

    println!("killed pool upserts: {answers:?}");
    assert_retryable_or_landed(&answers, &[201]);
    assert!(
        answers.iter().any(|answer| matches!(answer, Some((201, _)))),
        "the tenant must serve again after its pool is resumed: {answers:?}"
    );
    reset(&client, &pool).await;
    pool.close().await;
}

/// Clause 5.5.6 with RFC 7231 6.6.4, for a deployment of two pooled replicas
/// sharing one database and one pooler: both answer the outage 503 with
/// `Retry-After`, never 500, both serve again once the pool is resumed, and every write a client retried as told lands
/// exactly once, whichever replica the retry reaches; a write whose commit
/// outcome the outage left unknown is safe to retry because the upsert is
/// idempotent in the database.
#[tokio::test]
#[serial_test::serial]
async fn both_pooled_replicas_answer_an_outage_503_and_keep_every_acknowledged_write() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let replicas = [endpoints::broker(), endpoints::broker_replica()];
    let tenant = tenant_load::tenant_names("pgbkillrep", 1).remove(0);
    tenant_load::provision_tenants(&client, &replicas[0], slice::from_ref(&tenant), CORE_CONTEXT, 1)
        .await
        .expect("every tenant is provisioned");
    let provisioned = urlencoding::encode(&format!("urn:ngsi-ld:Device:provision:{tenant}")).into_owned();
    let warm = client
        .get(format!("{}entities/{provisioned}", replicas[1]))
        .header("NGSILD-Tenant", &tenant)
        .header("Link", link_header())
        .send()
        .await
        .expect("the replica answers");
    assert_eq!(warm.status().as_u16(), 200, "both replicas hold a warm runtime");
    let database = tenant_database(&pool).await;

    let drive = async {
        let mut answers: BTreeMap<String, (usize, Answer)> = BTreeMap::new();
        let started = Instant::now();
        let mut served = [false; 2];
        let mut sequence = 0_usize;
        loop {
            sequence += 1;
            let replica = sequence % replicas.len();
            let id = format!("urn:ngsi-ld:Device:replicated:{sequence}");
            let answer = upsert(&client, &replicas[replica], &tenant, &id).await;
            if matches!(answer, Some((201, _))) && started.elapsed() > KILLED_FOR * 2 {
                served[replica] = true;
            }
            answers.insert(id, (replica, answer));
            if outage_is_over(started, served.iter().all(|replica| *replica)) {
                return answers;
            }
            sleep(REQUEST_SPACING).await;
        }
    };
    let (answers, ()) = tokio::join!(drive, kill_then_resume(&database));

    println!("replicated outage upserts: {answers:?}");
    let first: Vec<Answer> = answers.values().map(|(_, answer)| answer.clone()).collect();
    assert_retryable_or_landed(&first, &[201]);

    // Retry every write that was not acknowledged through the other replica,
    // as often and as late as `Retry-After` says.
    for (id, (replica, answer)) in &answers {
        if matches!(answer, Some((201, _))) {
            continue;
        }
        let other = &replicas[(replica + 1) % replicas.len()];
        let mut retried = upsert(&client, other, &tenant, id).await;
        for _ in 0..10 {
            if matches!(retried, Some((201 | 204, _))) {
                break;
            }
            assert!(!matches!(retried, Some((500, _))), "a retry is never answered 500: {id} {retried:?}");
            sleep(Duration::from_secs(5)).await;
            retried = upsert(&client, other, &tenant, id).await;
        }
        assert!(matches!(retried, Some((201 | 204, _))), "a retried write lands: {id} {retried:?}");
    }

    for id in answers.keys() {
        let encoded = urlencoding::encode(id).into_owned();
        for replica in &replicas {
            let read = client
                .get(format!("{replica}entities/{encoded}"))
                .header("NGSILD-Tenant", &tenant)
                .header("Link", link_header())
                .send()
                .await
                .expect("the replica answers");
            assert_eq!(read.status().as_u16(), 200, "{id} is held, as {replica} reads it");
        }
    }
    reset(&client, &pool).await;
    pool.close().await;
}

/// The one remote entity the mock context source holds.
const REMOTE_ENTITY: &str = "urn:ngsi-ld:OutageProbe:remote";

/// Clause 6.3.17 with clause 5.5.6: a federated query whose sources cannot
/// be learnt, because this broker's own registration store is unreachable,
/// is a 503; it is never answered with the local data alone, which would
/// hide the source without a warning.
#[tokio::test]
#[serial_test::serial]
async fn a_killed_pool_never_answers_a_federated_read_with_local_data_only() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let tenant = tenant_load::tenant_names("pgbkillfed", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1)
        .await
        .expect("every tenant is provisioned");
    let source = ContextSourceAdapter::start().await;
    let registration = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:outage-probe",
        "type": "ContextSourceRegistration",
        "information": [{ "entities": [{ "type": "OutageProbe" }] }],
        "endpoint": source.url(""),
        "mode": "inclusive",
    });
    let registered = client
        .post(format!("{broker}csourceRegistrations"))
        .header("NGSILD-Tenant", &tenant)
        .header("Content-Type", "application/json")
        .header("Link", link_header())
        .json(&registration)
        .send()
        .await
        .expect("the registration is answered");
    assert_eq!(registered.status().as_u16(), 201, "the registration is created");
    let remote: Value = json!([{ "id": REMOTE_ENTITY, "type": "OutageProbe", "@context": CORE_CONTEXT.as_str() }]);
    let database = tenant_database(&pool).await;

    let drive = async {
        let mut answers = Vec::new();
        let started = Instant::now();
        loop {
            source.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &remote).await;
            let response = client
                .get(format!("{broker}entities?type=OutageProbe"))
                .header("NGSILD-Tenant", &tenant)
                .header("Link", link_header())
                .send()
                .await;
            let answer = match response {
                Ok(response) => {
                    let status = response.status().as_u16();
                    let body = response.text().await.unwrap_or_default();
                    Some((status, body))
                }
                Err(_) => None,
            };
            let served = matches!(answer, Some((200, _)));
            answers.push(answer);
            if outage_is_over(started, served) {
                return answers;
            }
            sleep(REQUEST_SPACING).await;
        }
    };
    let (answers, ()) = tokio::join!(drive, kill_then_resume(&database));

    println!("killed pool federated reads: {answers:?}");
    for answer in &answers {
        match answer {
            Some((200, body)) => assert!(body.contains(REMOTE_ENTITY), "a 200 without the remote entity hides the source: {body}"),
            Some((503, _)) | None => {}
            Some((status, body)) => panic!("an outage is answered 503, never {status}: {body}"),
        }
    }
    assert!(
        answers.iter().any(|answer| matches!(answer, Some((200, _)))),
        "the query must be answered again after the pool is resumed: {answers:?}"
    );
    source.shutdown().await;
    reset(&client, &pool).await;
    pool.close().await;
}
