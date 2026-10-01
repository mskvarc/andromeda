//! Resilience of the pooled request path to pooler churn: a reconnect under
//! load loses no request, a pause stalls then resumes cleanly, terminated
//! server backends are replaced, pool saturation is a 503 with Retry-After
//! rather than a 500 -- including when it is the name dictionary lookup that
//! finds the pool saturated -- a killed database pool recovers, and the outage
//! a killed pool causes is answered 503 rather than 500 ([`store_outage`]).
//!
//! Everything drives the pooled `broker` and the pooler admin console on the
//! `pgbouncer` stack.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

#[path = "../support/context_source_adapter.rs"]
pub mod context_source_adapter;
mod store_outage;

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::pooler_probe::{
    endpoints,
    pooler_console::PoolerConsole,
    request_outcome::{RequestOutcome, TransportFailure},
    tenant_cleanup,
    tenant_load,
    tenant_load::UniformPicker,
};
use reqwest::Client;
use sqlx::PgPool;
use std::{future::Future, slice, time::Duration};
use tokio::time::{Instant, sleep};

/// A client with a generous timeout so a hung request is measured, not cut short.
fn client() -> Client {
    Client::builder().timeout(Duration::from_secs(40)).build().expect("reqwest client")
}

/// The Link header naming the core context.
fn link_header() -> String {
    format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

/// Connect to the control database over the direct endpoint.
async fn control_pool() -> PgPool {
    PgPool::connect(&endpoints::database_url())
        .await
        .expect("connect to PostgreSQL control database")
}

/// Reset the tier before and after a test.
async fn reset(client: &Client, pool: &PgPool) {
    let brokers = [
        endpoints::broker(),
        endpoints::broker_replica(),
        endpoints::broker_wide(),
        endpoints::broker_direct(),
    ];
    let refs: Vec<&str> = brokers.iter().map(String::as_str).collect();
    let _ = tenant_cleanup::reset(client, &refs, pool).await;
}

/// GET the provisioned entity under `tenant`.
async fn get_provisioned(client: &Client, broker: &str, tenant: &str) -> RequestOutcome {
    let id = urlencoding::encode(&format!("urn:ngsi-ld:Device:provision:{tenant}")).into_owned();
    RequestOutcome::of(
        client
            .get(format!("{broker}entities/{id}"))
            .header("NGSILD-Tenant", tenant)
            .header("Accept", "application/ld+json")
            .header("Link", link_header()),
    )
    .await
}

/// Drive a uniform read workload while `disruption` runs concurrently, returning
/// the load report.
async fn load_with<Fut>(client: &Client, broker: &str, tenants: &[String], seconds: u64, disruption: Fut) -> tenant_load::LoadReport
where
    Fut: Future<Output = ()>,
{
    let picker = UniformPicker::new(tenants.len(), 99);
    let load = tenant_load::run_bounded(24, Duration::from_secs(seconds), |_| {
        let client = &*client;
        let broker = &*broker;
        let tenants = &*tenants;
        let picker = &picker;
        async move { get_provisioned(client, broker, &tenants[picker.pick()]).await }
    });
    let (report, ()) = tokio::join!(load, disruption);
    report
}

#[tokio::test]
#[serial_test::serial]
async fn reconnect_under_load_loses_no_request() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let tenants = tenant_load::tenant_names("pgbrecon", 16);
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 16)
        .await
        .expect("every tenant is provisioned");
    let console = PoolerConsole::new(endpoints::pgbouncer_admin_url());

    let report = load_with(&client, &broker, &tenants, 10, async {
        sleep(Duration::from_secs(3)).await;
        let _ = console.reconnect().await;
    })
    .await;

    println!(
        "reconnect under load: {} requests, {} non-2xx, statuses {:?}, transport failures {:?} {:?}",
        report.total, report.non_2xx, report.statuses, report.transport_failures, report.transport_samples
    );
    assert_eq!(report.non_2xx, 0, "a reconnect under load must lose no request: {:?}", report.error_details);
    reset(&client, &pool).await;
    pool.close().await;
}

#[tokio::test]
#[serial_test::serial]
async fn a_paused_pooler_stalls_then_resumes_requests() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let tenants = tenant_load::tenant_names("pgbpause", 16);
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 16)
        .await
        .expect("every tenant is provisioned");
    let console = PoolerConsole::new(endpoints::pgbouncer_admin_url());

    // The pause stays well inside the pooler's fifteen-second
    // query_wait_timeout, so every stalled transaction resumes rather than being
    // shed.
    let report = load_with(&client, &broker, &tenants, 12, async {
        sleep(Duration::from_secs(2)).await;
        let _ = console.pause().await;
        sleep(Duration::from_secs(3)).await;
        let _ = console.resume().await;
    })
    .await;

    println!(
        "paused pooler: {} requests, {} non-2xx, statuses {:?}, transport failures {:?} {:?}, max latency {:?}",
        report.total, report.non_2xx, report.statuses, report.transport_failures, report.transport_samples, report.max
    );
    assert_eq!(
        report.non_2xx, 0,
        "a pause then resume must complete every in-flight request: {:?}",
        report.error_details
    );
    assert!(
        report.max < Duration::from_secs(8),
        "max latency must stay within the pause plus a margin, saw {:?}",
        report.max
    );
    reset(&client, &pool).await;
    pool.close().await;
}

#[tokio::test]
#[serial_test::serial]
async fn terminated_server_backends_are_replaced() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let tenants = tenant_load::tenant_names("pgbterm", 16);
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 16)
        .await
        .expect("every tenant is provisioned");

    // Terminate every pooler-origin server backend mid-load; the pooler must
    // reconnect and requests recover.
    let terminator = {
        let pool = pool.clone();
        async move {
            sleep(Duration::from_secs(3)).await;
            let _ = sqlx::query(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE backend_type = 'client backend' AND datname LIKE 'andromeda_tenant_%'",
            )
            .execute(&pool)
            .await;
        }
    };
    let report = load_with(&client, &broker, &tenants, 10, terminator).await;

    // After the disruption, every tenant answers again.
    let mut recovered = 0;
    for tenant in &tenants {
        if get_provisioned(&client, &broker, tenant).await.is_success() {
            recovered += 1;
        }
    }
    println!(
        "terminated backends: {} requests, {} non-2xx during window, {}/{} tenants recovered",
        report.total,
        report.non_2xx,
        recovered,
        tenants.len()
    );
    assert_eq!(
        recovered,
        tenants.len(),
        "every tenant must answer after its server backend is terminated and replaced"
    );
    reset(&client, &pool).await;
    pool.close().await;
}

#[tokio::test]
#[serial_test::serial]
async fn pool_saturation_is_a_503_with_retry_after_never_a_500() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();

    // Provision the target alone so its database is the only tenant database,
    // then find its name and provision a bystander tenant.
    let target = tenant_load::tenant_names("pgbsat", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&target), CORE_CONTEXT, 1)
        .await
        .expect("every tenant is provisioned");
    let target_db: String = sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_%' LIMIT 1")
        .fetch_one(&pool)
        .await
        .expect("target tenant database exists");
    let bystander = tenant_load::tenant_names("pgbsatby", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&bystander), CORE_CONTEXT, 1)
        .await
        .expect("every tenant is provisioned");

    // Occupy the target database's whole pool (default_pool_size + reserve = 3)
    // with long sleeps opened through the pooler from the test.
    let pooler_target = endpoints::pgbouncer_admin_url().replace("/pgbouncer", &format!("/{target_db}"));
    let hog = PgPool::connect_lazy(&pooler_target).expect("lazy pooler pool");
    let mut sleeps = Vec::new();
    for _ in 0..3 {
        let hog = hog.clone();
        sleeps.push(tokio::spawn(async move { sqlx::query("SELECT pg_sleep(20)").execute(&hog).await }));
    }
    sleep(Duration::from_secs(1)).await;

    // Fire requests at the saturated tenant: every response must be a 2xx or a
    // 503 carrying Retry-After -- never a 500 and never a hang past the client
    // timeout.
    let mut statuses = Vec::new();
    let mut transport_failures = Vec::new();
    let mut retry_after_ok = true;
    for _ in 0..64 {
        match client
            .get(format!(
                "{broker}entities/{}",
                urlencoding::encode(&format!("urn:ngsi-ld:Device:provision:{target}")).into_owned()
            ))
            .header("NGSILD-Tenant", target.as_str())
            .header("Accept", "application/ld+json")
            .header("Link", link_header())
            .send()
            .await
        {
            Ok(response) => {
                let code = response.status().as_u16();
                if code == 503 && response.headers().get("Retry-After").is_none() {
                    retry_after_ok = false;
                }
                statuses.push(code);
            }
            Err(error) => transport_failures.push(TransportFailure::from_error(&error)),
        }
    }
    // The bystander tenant is unaffected.
    let bystander_status = get_provisioned(&client, &broker, &bystander).await;

    for handle in sleeps {
        let _ = handle.await;
    }
    hog.close().await;

    let server_errors = statuses.iter().filter(|code| **code >= 500 && **code != 503).count();
    println!("saturation: statuses {statuses:?}, 5xx-not-503 {server_errors}, transport failures {transport_failures:?}, bystander {bystander_status:?}");
    assert_eq!(server_errors, 0, "pool saturation must never surface a 500");
    assert!(
        transport_failures.is_empty(),
        "pool saturation must not hang or drop the request: {transport_failures:?}"
    );
    assert!(retry_after_ok, "every 503 must carry a Retry-After header");
    assert!(
        bystander_status.is_success(),
        "a bystander tenant must be unaffected by another tenant's saturation: {bystander_status:?}"
    );
    reset(&client, &pool).await;
    pool.close().await;
}

/// Query `path` under `tenant` for a type no entity carries, so the name
/// dictionary misses its cache and resolves it against the database -- the first
/// statement the request runs.
async fn query_uninterned_type(client: &Client, broker: &str, tenant: &str, path: &str) -> Result<(u16, bool), TransportFailure> {
    client
        .get(format!("{broker}{path}"))
        .header("NGSILD-Tenant", tenant)
        .header("Accept", "application/ld+json")
        .header("Link", link_header())
        .send()
        .await
        .map(|response| (response.status().as_u16(), response.headers().contains_key("Retry-After")))
        .map_err(|error| TransportFailure::from_error(&error))
}

#[tokio::test]
#[serial_test::serial]
async fn a_saturated_name_dictionary_lookup_is_a_503_never_a_500() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();

    let target = tenant_load::tenant_names("pgbdict", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&target), CORE_CONTEXT, 1)
        .await
        .expect("every tenant is provisioned");
    let target_db: String = sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_%' LIMIT 1")
        .fetch_one(&pool)
        .await
        .expect("target tenant database exists");

    // Occupy the target database's whole pooler pool (default_pool_size +
    // reserve = 3) for longer than the pooler's fifteen-second
    // query_wait_timeout, so a statement queued behind it is shed.
    let pooler_target = endpoints::pgbouncer_admin_url().replace("/pgbouncer", &format!("/{target_db}"));
    let hog = PgPool::connect_lazy(&pooler_target).expect("lazy pooler pool");
    let mut sleeps = Vec::new();
    for _ in 0..3 {
        let hog = hog.clone();
        sleeps.push(tokio::spawn(async move { sqlx::query("SELECT pg_sleep(20)").execute(&hog).await }));
    }
    sleep(Duration::from_secs(1)).await;

    // The type was never written, so no dictionary cache holds it: both the
    // entity and the temporal query resolve it on the saturated pool before
    // their own statement runs.
    let paths = [
        "entities?type=UninternedDictionaryProbe",
        "entities?type=UninternedDictionaryProbe&q=neverWritten==1",
        "entities?type=UninternedDictionaryProbe&count=true",
        "temporal/entities?type=UninternedDictionaryProbe&timerel=before&timeAt=2100-01-01T00:00:00Z",
        "temporal/entities?type=UninternedDictionaryProbe&q=neverWritten==1&timerel=before&timeAt=2100-01-01T00:00:00Z",
    ];
    let requests: Vec<_> = paths
        .into_iter()
        .map(|path| {
            let client = client.clone();
            let broker = broker.clone();
            let target = target.clone();
            tokio::spawn(async move { query_uninterned_type(&client, &broker, &target, path).await })
        })
        .collect();
    let mut outcomes = Vec::new();
    for request in requests {
        outcomes.push(request.await.expect("request task completes"));
    }

    for handle in sleeps {
        let _ = handle.await;
    }
    hog.close().await;

    println!("saturated dictionary lookups: {outcomes:?}");
    let answered: Vec<(u16, bool)> = outcomes.iter().filter_map(|outcome| outcome.as_ref().ok().copied()).collect();
    assert_eq!(answered.len(), paths.len(), "no lookup may hang or drop the request: {outcomes:?}");
    assert!(
        answered.iter().all(|(status, _)| *status == 200 || *status == 503),
        "a saturated name dictionary lookup must be a 2xx or a 503, never a 500: {answered:?}"
    );
    assert!(
        answered.iter().any(|(status, _)| *status == 503),
        "every lookup queued behind the saturated pool, so at least one must have been shed: {answered:?}"
    );
    assert!(
        answered.iter().filter(|(status, _)| *status == 503).all(|(_, retry_after)| *retry_after),
        "every 503 must carry a Retry-After header: {answered:?}"
    );
    reset(&client, &pool).await;
    pool.close().await;
}

#[tokio::test]
#[serial_test::serial]
async fn a_killed_database_pool_recovers() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let tenant = tenant_load::tenant_names("pgbkill", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1)
        .await
        .expect("every tenant is provisioned");
    let target_db: String = sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_%' LIMIT 1")
        .fetch_one(&pool)
        .await
        .expect("tenant database exists");

    let console = PoolerConsole::new(endpoints::pgbouncer_admin_url());
    let _ = console.kill(&target_db).await;
    sleep(Duration::from_secs(1)).await;
    let _ = console.resume_database(&target_db).await;

    // PgBouncer caches a killed pool's login failure for a few seconds, so the
    // first request after RESUME may still be shed as 503 backpressure. Recovery
    // is "serves again", not "serves instantly": poll until a 2xx within a bound,
    // never a 5xx-other-than-503 and never a hang.
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut outcome = get_provisioned(&client, &broker, &tenant).await;
    while !outcome.is_success() && Instant::now() < deadline {
        // A request in flight when the pool was killed may lose its connection;
        // any answer must still be 503 backpressure, never another 5xx.
        assert!(
            matches!(outcome.status(), Some(503) | None),
            "a killed pool must recover through 503 backpressure, never another 5xx, saw {outcome:?}"
        );
        sleep(Duration::from_millis(500)).await;
        outcome = get_provisioned(&client, &broker, &tenant).await;
    }
    println!("killed database pool: post-recovery outcome {outcome:?}");
    assert!(
        outcome.is_success(),
        "a tenant must serve again after its pool is killed and resumed, saw {outcome:?}"
    );
    reset(&client, &pool).await;
    pool.close().await;
}
