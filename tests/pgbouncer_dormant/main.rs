//! Spec obligations of tenants that are not in the runtime cache: a
//! time-interval subscription keeps notifying after eviction, a failed durable
//! delivery is retried after eviction, an entity expires on a dormant tenant, a
//! tenant with nothing due stays dormant, and a busy outbox keeps its tenant
//! serviced. Every test evicts the target by cache pressure -- touching more
//! than the runtime cache holds -- then observes the dormant tenant.
//!
//! Drives the pooled `broker` (cache 32) on the `pgbouncer` stack.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::pooler_probe::{backend_sampler, endpoints, metrics_scrape::Metrics, tenant_cleanup, tenant_load};
use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::{CallbackServer, MockResponse, RequestMatcher},
    notification_server::NotificationServer,
    server_config::MockServerConfig,
};
use chrono::{SecondsFormat, Utc};
use reqwest::{Client, Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use std::{slice, time::Duration};
use tokio::time::{Instant, sleep};

/// How many other tenants to touch to force the target out of the cache of 32.
const EVICTION_PRESSURE: usize = 40;

/// A client with a generous timeout.
fn client() -> Client {
    Client::builder().timeout(Duration::from_secs(30)).build().expect("reqwest client")
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

/// Provision and touch `EVICTION_PRESSURE` fresh tenants so the target, now the
/// least-recently-used, is evicted from the cache.
async fn evict_by_pressure(client: &Client, broker: &str) {
    let others = tenant_load::tenant_names("pgbevict", EVICTION_PRESSURE);
    tenant_load::provision_tenants(client, broker, &others, CORE_CONTEXT, 16).await;
    for tenant in &others {
        let id = urlencoding::encode(&format!("urn:ngsi-ld:Device:provision:{tenant}")).into_owned();
        let _ = client
            .get(format!("{broker}entities/{id}"))
            .header("NGSILD-Tenant", tenant.as_str())
            .send()
            .await;
    }
}

/// The single provisioned tenant database's name.
async fn only_tenant_database(pool: &PgPool) -> String {
    sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_%' ORDER BY datname LIMIT 1")
        .fetch_one(pool)
        .await
        .expect("a tenant database exists")
}

#[tokio::test]
#[serial_test::serial]
async fn a_time_interval_subscription_keeps_notifying_after_its_tenant_is_evicted() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let server = NotificationServer::start(MockServerConfig::from_environment().expect("mock config"))
        .await
        .expect("notification server");
    let callback = server.endpoint_url().expect("callback url");
    let tenant = tenant_load::tenant_names("pgbinterval", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1).await;

    // Clause 5.8.6: a periodic notification every five seconds.
    let _ = client
        .post(format!("{broker}subscriptions"))
        .header("NGSILD-Tenant", tenant.as_str())
        .header("Content-Type", "application/ld+json")
        .json(&json!({
            "type": "Subscription",
            "entities": [{"type": "Device"}],
            "notification": {"endpoint": {"uri": callback, "accept": "application/json"}},
            "timeInterval": 5,
            "@context": [CORE_CONTEXT]
        }))
        .send()
        .await;

    evict_by_pressure(&client, &broker).await;

    // Count periodic notifications over 30s without ever touching the tenant.
    let mut count = 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if server.next_request(Duration::from_secs(6)).await.is_ok() {
            count += 1;
        }
    }
    println!("interval after eviction: {count} notifications in 30s");
    server.shutdown().await.expect("shutdown");
    assert!(
        count >= 4,
        "a time-interval subscription must keep notifying after its tenant is evicted, saw {count}"
    );
    reset(&client, &pool).await;
    pool.close().await;
}

#[tokio::test]
#[serial_test::serial]
async fn a_failed_durable_delivery_is_retried_after_eviction() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let server = CallbackServer::start(MockServerConfig::from_environment().expect("mock config"))
        .await
        .expect("callback server");
    let path = CallbackPath::parse("/dormant-retry").expect("callback path");
    let matcher = RequestMatcher::new(Method::POST, path.clone());
    // The first delivery fails, the retry succeeds.
    server
        .enqueue(matcher.clone(), MockResponse::builder().status(StatusCode::INTERNAL_SERVER_ERROR).build())
        .await;
    server.enqueue(matcher.clone(), MockResponse::builder().status(StatusCode::OK).build()).await;
    let callback = server.endpoint(&path).expect("callback endpoint").to_string();

    let tenant = tenant_load::tenant_names("pgbretry", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1).await;
    subscribe_change(&client, &broker, &tenant, &callback).await;

    // Trigger a change: the first delivery attempt gets a 500.
    let _ = client
        .patch(format!(
            "{broker}entities/{}/attrs",
            urlencoding::encode(&format!("urn:ngsi-ld:Device:provision:{tenant}")).into_owned()
        ))
        .header("NGSILD-Tenant", tenant.as_str())
        .header("Content-Type", "application/ld+json")
        .json(&json!({"value": {"type": "Property", "value": 7}, "@context": [CORE_CONTEXT]}))
        .send()
        .await;
    sleep(Duration::from_secs(2)).await;
    evict_by_pressure(&client, &broker).await;

    // The durable retry must arrive after eviction, without touching the tenant.
    // A failed delivery holds its claim lease for the full term (30s by default,
    // preventing a second worker double-delivering), so the retry is gated by the
    // lease, not the shorter backoff -- the dormant wake fires only once the lease
    // lapses. The window therefore allows for the lease term plus the wake chain
    // (probe cadence, admission, the drain's first pass), not merely the backoff.
    let mut attempts = 0;
    let deadline = Instant::now() + Duration::from_secs(75);
    while Instant::now() < deadline {
        if server.next_request(Duration::from_secs(6)).await.is_ok() {
            attempts += 1;
        }
        if attempts >= 2 {
            break;
        }
    }
    println!("durable retry after eviction: {attempts} delivery attempts");
    server.shutdown().await.expect("shutdown");
    assert!(
        attempts >= 2,
        "a failed durable delivery must be retried after the tenant is evicted, saw {attempts}"
    );
    reset(&client, &pool).await;
    pool.close().await;
}

/// Create a change subscription for `tenant` notifying `callback`.
async fn subscribe_change(client: &Client, broker: &str, tenant: &str, callback: &str) {
    let _ = client
        .post(format!("{broker}subscriptions"))
        .header("NGSILD-Tenant", tenant)
        .header("Content-Type", "application/ld+json")
        .json(&json!({
            "type": "Subscription",
            "entities": [{"type": "Device"}],
            "notification": {"endpoint": {"uri": callback, "accept": "application/json"}},
            "@context": [CORE_CONTEXT]
        }))
        .send()
        .await;
}

#[tokio::test]
#[serial_test::serial]
async fn an_entity_expires_on_a_dormant_tenant() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let tenant = tenant_load::tenant_names("pgbexpire", 1).remove(0);

    // Clause 4.22: an entity whose expiresAt is twenty seconds out.
    let expires_at = (Utc::now() + chrono::Duration::seconds(20)).to_rfc3339_opts(SecondsFormat::Secs, true);
    let _ = client
        .post(format!("{broker}entities"))
        .header("NGSILD-Tenant", tenant.as_str())
        .header("Content-Type", "application/ld+json")
        .json(&json!({
            "id": "urn:ngsi-ld:Device:ephemeral",
            "type": "Device",
            "expiresAt": expires_at,
            "value": {"type": "Property", "value": 1},
            "@context": [CORE_CONTEXT]
        }))
        .send()
        .await;
    let database = only_tenant_database(&pool).await;
    let cold_before = Metrics::scrape(&client, &broker)
        .await
        .map_or(0.0, |m| m.sum("andromeda_tenant_cold_builds_total"));

    evict_by_pressure(&client, &broker).await;
    sleep(Duration::from_secs(60)).await;

    // The entity is gone, and no request reloaded the tenant to make it so: the
    // cold-build counter did not advance for it (a dormant sweep did the work).
    let remaining = count_entities(&database).await;
    let cold_after = Metrics::scrape(&client, &broker)
        .await
        .map_or(0.0, |m| m.sum("andromeda_tenant_cold_builds_total"));
    println!(
        "dormant expiry: {remaining} entities remain in {database}, cold builds delta {}",
        cold_after - cold_before
    );
    assert_eq!(remaining, 0, "an expired entity must be swept on a dormant tenant, {remaining} remain");
    reset(&client, &pool).await;
    pool.close().await;
}

/// Count entities in a tenant database over a direct connection.
async fn count_entities(database: &str) -> i64 {
    // Swap only the trailing database segment; a blind replace of "/postgres"
    // would also rewrite the "//postgres" user-info and connect as the wrong role.
    let base = endpoints::database_url();
    let url = match base.rsplit_once('/') {
        Some((prefix, _)) => format!("{prefix}/{database}"),
        None => base,
    };
    let tenant_pool = match PgPool::connect(&url).await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("count_entities connect to {database} failed: {error}");
            return -1;
        }
    };
    let count = match sqlx::query_scalar::<_, i64>("SELECT count(*) FROM entities").fetch_one(&tenant_pool).await {
        Ok(count) => count,
        Err(error) => {
            eprintln!("count_entities query on {database} failed: {error}");
            -1
        }
    };
    tenant_pool.close().await;
    count
}

#[tokio::test]
#[serial_test::serial]
async fn a_tenant_with_nothing_due_stays_dormant() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let tenant = tenant_load::tenant_names("pgbquiet", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1).await;
    let database = only_tenant_database(&pool).await;

    let loaded_before = Metrics::scrape(&client, &broker)
        .await
        .ok()
        .and_then(|m| m.value("andromeda_tenant_wakes_total", &[("outcome", "loaded")]))
        .unwrap_or(0.0);
    evict_by_pressure(&client, &broker).await;
    sleep(Duration::from_secs(60)).await;
    let loaded_after = Metrics::scrape(&client, &broker)
        .await
        .ok()
        .and_then(|m| m.value("andromeda_tenant_wakes_total", &[("outcome", "loaded")]))
        .unwrap_or(0.0);
    let backends = backend_sampler::client_backends_for(&pool, &database).await.unwrap_or(-1);

    println!(
        "nothing-due tenant: wakes(loaded) delta {}, backends for its database {backends}",
        loaded_after - loaded_before
    );
    assert!(
        (loaded_after - loaded_before) < 1.0,
        "a tenant with nothing due must not be woken, saw {} wakes",
        loaded_after - loaded_before
    );
    assert_eq!(backends, 0, "a dormant tenant with nothing due must hold no backends, saw {backends}");
    reset(&client, &pool).await;
    pool.close().await;
}

#[tokio::test]
#[serial_test::serial]
async fn a_busy_outbox_keeps_its_tenant_serviced_after_eviction() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let server = CallbackServer::start(MockServerConfig::from_environment().expect("mock config"))
        .await
        .expect("callback server");
    let path = CallbackPath::parse("/dormant-busy").expect("callback path");
    let callback = server.endpoint(&path).expect("callback endpoint").to_string();
    let tenant = tenant_load::tenant_names("pgbbusy", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1).await;
    subscribe_change(&client, &broker, &tenant, &callback).await;

    // Every delivery fails, so the outbox keeps retrying: a busy outbox with no
    // requests to the tenant. No responses are enqueued, so the server's default
    // is used; the point is that delivery keeps being attempted.
    for value in 0..5 {
        let _ = client
            .patch(format!(
                "{broker}entities/{}/attrs",
                urlencoding::encode(&format!("urn:ngsi-ld:Device:provision:{tenant}")).into_owned()
            ))
            .header("NGSILD-Tenant", tenant.as_str())
            .header("Content-Type", "application/ld+json")
            .json(&json!({"value": {"type": "Property", "value": value}, "@context": [CORE_CONTEXT]}))
            .send()
            .await;
    }
    let _ = server.drain_requests().await;
    evict_by_pressure(&client, &broker).await;

    // After eviction, delivery attempts must keep arriving: the busy outbox kept
    // the tenant serviced rather than being idled out.
    let mut after_eviction = 0;
    let deadline = Instant::now() + Duration::from_secs(40);
    while Instant::now() < deadline {
        if server.next_request(Duration::from_secs(6)).await.is_ok() {
            after_eviction += 1;
        }
    }
    println!("busy outbox after eviction: {after_eviction} delivery attempts");
    server.shutdown().await.expect("shutdown");
    assert!(
        after_eviction >= 1,
        "a busy outbox must keep its tenant serviced after eviction, saw {after_eviction} attempts"
    );
    reset(&client, &pool).await;
    pool.close().await;
}

/// Suspend a tenant through the tenant-management extension.
async fn suspend(client: &Client, broker: &str, tenant: &str, is_active: bool) {
    let status = client
        .patch(format!("{broker}tenants/{tenant}"))
        .header("Content-Type", "application/json")
        .body(json!({"isActive": is_active}).to_string())
        .send()
        .await
        .expect("suspend/resume request")
        .status();
    assert_eq!(status, StatusCode::NO_CONTENT, "PATCH isActive must return 204, got {status}");
}

#[tokio::test]
#[serial_test::serial]
async fn suspending_a_tenant_closes_its_backends_synchronously() {
    // The deterministic replacement for the retired orphan-eviction sweep: a
    // suspend evicts the tenant's runtime at once, so its backends drop without
    // waiting for a janitor tick. The janitor never touches it -- it is still
    // registered (status suspended), so it is neither an orphan nor idle -- so a
    // prompt drop to zero can only be the suspend's own synchronous eviction.
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let tenant = tenant_load::tenant_names("pgbsuspend", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1).await;

    // Touch the tenant so a runtime and its pool are resident.
    let id = urlencoding::encode(&format!("urn:ngsi-ld:Device:provision:{tenant}")).into_owned();
    let _ = client
        .get(format!("{broker}entities/{id}"))
        .header("NGSILD-Tenant", tenant.as_str())
        .send()
        .await;
    let database = only_tenant_database(&pool).await;
    let before = backend_sampler::client_backends_for(&pool, &database).await.unwrap_or(0);
    assert!(before > 0, "an active tenant holds backends, saw {before}");

    suspend(&client, &broker, &tenant, false).await;

    // Poll briefly for the detached pool close to settle -- well inside a janitor
    // interval, and the janitor would not evict a suspended-but-registered tenant.
    let mut after = backend_sampler::client_backends_for(&pool, &database).await.unwrap_or(0);
    let deadline = Instant::now() + Duration::from_secs(5);
    while after > 0 && Instant::now() < deadline {
        sleep(Duration::from_millis(200)).await;
        after = backend_sampler::client_backends_for(&pool, &database).await.unwrap_or(0);
    }
    assert_eq!(after, 0, "a suspend closes the tenant's backends synchronously, saw {after}");

    reset(&client, &pool).await;
    pool.close().await;
}

#[tokio::test]
#[serial_test::serial]
async fn a_suspended_tenant_is_inert_until_resumed() {
    // Contrast with the dormant-wake tests above: an evicted-but-active tenant's
    // interval subscription keeps firing, but a suspended tenant's must not -- the
    // waker skips it entirely. Once resumed and next accessed, its obligations run
    // again.
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let server = NotificationServer::start(MockServerConfig::from_environment().expect("mock config"))
        .await
        .expect("notification server");
    let callback = server.endpoint_url().expect("callback url");
    let tenant = tenant_load::tenant_names("pgbsuspinert", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1).await;

    // A periodic notification every five seconds (clause 5.8.6).
    let _ = client
        .post(format!("{broker}subscriptions"))
        .header("NGSILD-Tenant", tenant.as_str())
        .header("Content-Type", "application/ld+json")
        .json(&json!({
            "type": "Subscription",
            "entities": [{"type": "Device"}],
            "notification": {"endpoint": {"uri": callback, "accept": "application/json"}},
            "timeInterval": 5,
            "@context": [CORE_CONTEXT]
        }))
        .send()
        .await;

    suspend(&client, &broker, &tenant, false).await;
    // Drain any in-flight notification, then require silence: the waker must not
    // drain a suspended tenant.
    let _ = server.next_request(Duration::from_secs(1)).await;
    let mut while_suspended = 0;
    let deadline = Instant::now() + Duration::from_secs(18);
    while Instant::now() < deadline {
        if server.next_request(Duration::from_secs(6)).await.is_ok() {
            while_suspended += 1;
        }
    }
    assert_eq!(
        while_suspended, 0,
        "a suspended tenant's interval subscription must not be drained, saw {while_suspended}"
    );

    // Resume and touch so the runtime cold-loads and its drains re-arm.
    suspend(&client, &broker, &tenant, true).await;
    let id = urlencoding::encode(&format!("urn:ngsi-ld:Device:provision:{tenant}")).into_owned();
    let _ = client
        .get(format!("{broker}entities/{id}"))
        .header("NGSILD-Tenant", tenant.as_str())
        .send()
        .await;

    let mut after_resume = 0;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if server.next_request(Duration::from_secs(6)).await.is_ok() {
            after_resume += 1;
            break;
        }
    }
    server.shutdown().await.expect("shutdown");
    assert!(after_resume >= 1, "a resumed tenant's interval subscription runs again, saw {after_resume}");

    reset(&client, &pool).await;
    pool.close().await;
}
