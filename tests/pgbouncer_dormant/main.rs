//! Spec obligations of tenants that are not in the runtime cache: a
//! time-interval subscription keeps notifying after eviction, a failed durable
//! delivery is retried after eviction, an entity expires on a dormant tenant, a
//! tenant with nothing due stays dormant, and a busy outbox keeps its tenant
//! serviced. Every test evicts the target by cache pressure -- touching more
//! than the runtime cache holds -- then observes the dormant tenant.
//!
//! Drives the pooled `broker` (cache 32) on the `pgbouncer` stack; the suspend
//! tests also drive the un-pooled `broker-direct`, whose connections are the
//! PostgreSQL backends themselves.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_tenancy::database_name::TenantDatabaseName;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::pooler_probe::{
    backend_sampler,
    endpoints,
    metrics_scrape::Metrics,
    pooler_console::PoolerConsole,
    tenant_cleanup,
    tenant_database::{connect_database, tenant_database_name},
    tenant_load,
};
use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::{CallbackServer, MockResponse, RequestMatcher},
    notification_server::NotificationServer,
    server_config::MockServerConfig,
};
use chrono::{SecondsFormat, Utc};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{slice, time::Duration};
use tokio::time::{Instant, sleep};

/// How many other tenants to touch to force the target out of the cache of 32.
const EVICTION_PRESSURE: usize = 40;

/// The longest the target is watched for before it must have been evicted: the
/// eviction pressure itself, plus the residency a runtime the wake loop loaded
/// in the meantime is guaranteed (`tenancy.wake_residency`, 60 s) before the
/// janitor lets it go again.
const DORMANCY_DEADLINE: Duration = Duration::from_secs(150);

/// How often the target's residency is read while it is watched.
const DORMANCY_POLL: Duration = Duration::from_millis(100);

/// How long after the target was seen dormant its callback count is taken. A
/// delivery the evicted runtime had already sent lands within it, while a wake
/// cannot load the tenant before `tenancy.min_dormancy` (3 s on this stack) has
/// passed since the eviction.
const IN_FLIGHT_GRACE: Duration = Duration::from_secs(1);

/// The base retry backoff of a failed delivery on this stack
/// (`notifications.delivery.base_backoff`, the 5 s default).
const BASE_BACKOFF: Duration = Duration::from_secs(5);

/// The longest the first delivery attempt of a change is waited for.
const FIRST_ATTEMPT_DEADLINE: Duration = Duration::from_secs(30);

/// What a due retry of a dormant tenant waits for beyond its due instant: the
/// minimum dormancy (3 s), the janitor's wake cadence (5 s), the cold build,
/// the drain's first pass and one HTTP round trip, with a wide margin.
const WAKE_MARGIN: Duration = Duration::from_secs(30);

mod snapshot_branch_retry;

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

/// Reset the tier before and after a test, failing the test when the tier
/// cannot be brought back to no tenants: a scenario must never run over what
/// another left behind.
async fn reset(client: &Client, pool: &PgPool) {
    let brokers = [
        endpoints::broker(),
        endpoints::broker_replica(),
        endpoints::broker_wide(),
        endpoints::broker_direct(),
    ];
    let refs: Vec<&str> = brokers.iter().map(String::as_str).collect();
    tenant_cleanup::reset(client, &refs, pool)
        .await
        .expect("the tier resets to no provisioned tenant");
}

/// Provision and touch `EVICTION_PRESSURE` fresh tenants so the target, now the
/// least-recently-used, is evicted from the cache.
async fn evict_by_pressure(client: &Client, broker: &str) {
    let others = tenant_load::tenant_names("pgbevict", EVICTION_PRESSURE);
    tenant_load::provision_tenants(client, broker, &others, CORE_CONTEXT, 16)
        .await
        .expect("every tenant is provisioned");
    for tenant in &others {
        let id = urlencoding::encode(&format!("urn:ngsi-ld:Device:provision:{tenant}")).into_owned();
        let _ = client
            .get(format!("{broker}entities/{id}"))
            .header("NGSILD-Tenant", tenant.as_str())
            .send()
            .await;
    }
}

/// Scrape the pooled broker's metrics, failing the test when the exposition
/// cannot be read: an unreadable scrape must never pass as a zero reading.
async fn scrape(client: &Client, broker: &str) -> Metrics {
    Metrics::scrape(client, broker)
        .await
        .map_err(|error| format!("the broker at {broker} must expose its metrics: {error}"))
        .expect("an unreadable scrape must never pass as a zero")
}

/// The name of the database the platform registry holds `tenant` in.
async fn tenant_database(pool: &PgPool, tenant: &str) -> TenantDatabaseName {
    let tenant: TenantName = tenant.parse().expect("a provisioned tenant has a valid name");
    tenant_database_name(pool, &tenant).await.expect("the provisioned tenant is registered")
}

/// Watch `tenant` on `broker` until its tenant-management resource reports it
/// `dormant` -- in service, with no runtime resident. Reading the resource
/// does not load the tenant.
///
/// Fails the test when the tenant is not seen dormant within
/// [`DORMANCY_DEADLINE`]: a scenario about a dormant tenant must never pass on
/// one that stayed resident.
async fn wait_until_dormant(client: &Client, broker: &str, tenant: &str) {
    let deadline = Instant::now() + DORMANCY_DEADLINE;
    loop {
        let resource: Value = client
            .get(format!("{broker}tenants/{tenant}"))
            .send()
            .await
            .expect("the tenant resource is answered")
            .json()
            .await
            .expect("the tenant resource is JSON");
        if resource["status"] == "dormant" {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{tenant} is evicted from {broker} before the deadline, last seen as {resource}"
        );
        sleep(DORMANCY_POLL).await;
    }
}

/// When a failing delivery chain made its first attempt: the instant `server`
/// received it.
///
/// The stack permits ten attempts (`notifications.delivery.max_attempts` in
/// its compose file), and attempt `k` is made about `BASE_BACKOFF * (2^(k-1) -
/// 1)` after the first, so the chain keeps retrying for about 42 minutes --
/// far beyond [`DORMANCY_DEADLINE`], however slowly the eviction pressure
/// runs.
async fn first_attempt(server: &CallbackServer) -> Instant {
    server
        .next_request(FIRST_ATTEMPT_DEADLINE)
        .await
        .expect("the change's first delivery is attempted before the deadline");
    Instant::now()
}

/// The target of a failing delivery chain, evicted and seen dormant.
struct Dormancy {
    /// The delivery attempts `server` had received once the tenant was seen
    /// dormant and every delivery its evicted runtime had already sent landed.
    attempts: usize,
    /// When the tenant was seen dormant.
    seen_at: Instant,
}

/// Evict `tenant` from `broker` by cache pressure and wait until it is seen
/// dormant.
///
/// Every attempt `server` receives beyond [`Dormancy::attempts`] was made after
/// the eviction by a runtime only the wake loop can have loaded: nothing here
/// touches the tenant again.
async fn evict_until_dormant(client: &Client, broker: &str, tenant: &str, server: &CallbackServer) -> Dormancy {
    tokio::join!(wait_until_dormant(client, broker, tenant), evict_by_pressure(client, broker));
    let seen_at = Instant::now();
    sleep(IN_FLIGHT_GRACE).await;
    Dormancy {
        attempts: server.total_request_count(),
        seen_at,
    }
}

/// Wait until `server` has received an attempt beyond those counted in
/// `dormancy` and return how many it received in all, or the count in
/// `dormancy` when no further attempt arrived in time.
///
/// The wait is bounded by the retry chain itself rather than by a guess at how
/// long the eviction took. Each retry interval doubles the one before, so the
/// interval running when the tenant was seen dormant, `elapsed` after the first
/// attempt, is at most `elapsed + BASE_BACKOFF`: the next retry falls due no
/// later than that after dormancy, and a wake serves it within [`WAKE_MARGIN`].
async fn attempts_after_dormancy(server: &CallbackServer, first_attempt: Instant, dormancy: &Dormancy) -> usize {
    let elapsed = dormancy.seen_at.saturating_duration_since(first_attempt);
    let deadline = dormancy.seen_at + elapsed + BASE_BACKOFF + WAKE_MARGIN;
    while server.total_request_count() <= dormancy.attempts && Instant::now() < deadline {
        let _next = server.next_request(deadline.saturating_duration_since(Instant::now())).await;
    }
    server.total_request_count()
}

/// Queue one repeating 500 on `path`: every delivery attempt fails and is
/// retried.
async fn fail_every_attempt(server: &CallbackServer, path: &CallbackPath) {
    server
        .enqueue(
            RequestMatcher::new(Method::POST, path.clone()),
            MockResponse::builder().status(StatusCode::INTERNAL_SERVER_ERROR).build(),
        )
        .await;
}

/// Assert that a failing delivery chain was attempted before its tenant's
/// eviction and retried after it, naming `what` was retried.
fn assert_retried_after_dormancy(what: &str, dormancy: &Dormancy, after: usize) {
    println!("{what} after eviction: {} attempts before dormancy, {after} in all", dormancy.attempts);
    assert!(
        after > dormancy.attempts,
        "{what} must be retried after its tenant was evicted, saw no attempt beyond the {} before",
        dormancy.attempts
    );
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
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1)
        .await
        .expect("every tenant is provisioned");

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

/// A delivery that failed is retried when due even after its tenant was
/// evicted (clause 5.8.6: a notification is owed until it is delivered or
/// retired): the wake loop reads the retry from the tenant database and loads
/// the tenant for it.
///
/// Every attempt fails, so the retries keep coming at the stack's backoff for
/// longer than any eviction can take; the test requires one made after the
/// tenant was seen dormant. A wake that did not cover the delivery queue would
/// never load the tenant again, and no attempt would follow.
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
    fail_every_attempt(&server, &path).await;
    let callback = server.endpoint(&path).expect("callback endpoint").to_string();

    let tenant = tenant_load::tenant_names("pgbretry", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1)
        .await
        .expect("every tenant is provisioned");
    subscribe_change(&client, &broker, &tenant, &callback).await;

    // A change: its first delivery attempt gets a 500.
    let patched = client
        .patch(format!(
            "{broker}entities/{}/attrs",
            urlencoding::encode(&format!("urn:ngsi-ld:Device:provision:{tenant}")).into_owned()
        ))
        .header("NGSILD-Tenant", tenant.as_str())
        .header("Content-Type", "application/ld+json")
        .json(&json!({"value": {"type": "Property", "value": 7}, "@context": [CORE_CONTEXT]}))
        .send()
        .await
        .expect("the change is answered");
    assert_eq!(patched.status(), StatusCode::NO_CONTENT, "the change lands");

    let first = first_attempt(&server).await;
    let dormancy = evict_until_dormant(&client, &broker, &tenant, &server).await;
    let after = attempts_after_dormancy(&server, first, &dormancy).await;
    server.shutdown().await.expect("shutdown");
    assert_retried_after_dormancy("a failed durable delivery", &dormancy, after);
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
    let database = tenant_database(&pool, &tenant).await;
    let cold_before = scrape(&client, &broker).await.sum("andromeda_tenant_cold_builds_total");

    evict_by_pressure(&client, &broker).await;
    sleep(Duration::from_secs(60)).await;

    // The entity is gone, and no request reloaded the tenant to make it so: the
    // cold-build counter did not advance for it (a dormant sweep did the work).
    let remaining = count_entities(&database).await;
    let cold_after = scrape(&client, &broker).await.sum("andromeda_tenant_cold_builds_total");
    println!(
        "dormant expiry: {remaining} entities remain in {database}, cold builds delta {}",
        cold_after - cold_before
    );
    assert_eq!(remaining, 0, "an expired entity must be swept on a dormant tenant, {remaining} remain");
    reset(&client, &pool).await;
    pool.close().await;
}

/// Count entities in a tenant database over a direct connection.
async fn count_entities(database: &TenantDatabaseName) -> i64 {
    let tenant_pool = match connect_database(database).await {
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
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1)
        .await
        .expect("every tenant is provisioned");
    let database = tenant_database(&pool, &tenant).await;

    // A counter series appears only once its first increment is recorded, so an
    // absent `loaded` series in a readable exposition is a genuine zero.
    let loaded_before = scrape(&client, &broker)
        .await
        .value("andromeda_tenant_wakes_total", &[("outcome", "loaded")])
        .unwrap_or(0.0);
    evict_by_pressure(&client, &broker).await;
    sleep(Duration::from_secs(60)).await;
    let loaded_after = scrape(&client, &broker)
        .await
        .value("andromeda_tenant_wakes_total", &[("outcome", "loaded")])
        .unwrap_or(0.0);
    let backends = backend_sampler::client_backends_for(&pool, database.as_str()).await.unwrap_or(-1);

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

/// A busy outbox -- several changes whose deliveries keep failing -- keeps its
/// tenant serviced after eviction: the wake loop loads the tenant for the
/// retries without any request touching it.
///
/// Every attempt fails, so the retries keep coming at the stack's backoff for
/// longer than any eviction can take; the test requires one made after the
/// tenant was seen dormant. A tenant idled out with its outbox still due would
/// never be loaded again, and no attempt would follow.
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
    fail_every_attempt(&server, &path).await;
    let callback = server.endpoint(&path).expect("callback endpoint").to_string();
    let tenant = tenant_load::tenant_names("pgbbusy", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1)
        .await
        .expect("every tenant is provisioned");
    subscribe_change(&client, &broker, &tenant, &callback).await;

    for value in 0..5 {
        let patched = client
            .patch(format!(
                "{broker}entities/{}/attrs",
                urlencoding::encode(&format!("urn:ngsi-ld:Device:provision:{tenant}")).into_owned()
            ))
            .header("NGSILD-Tenant", tenant.as_str())
            .header("Content-Type", "application/ld+json")
            .json(&json!({"value": {"type": "Property", "value": value}, "@context": [CORE_CONTEXT]}))
            .send()
            .await
            .expect("the change is answered");
        assert_eq!(patched.status(), StatusCode::NO_CONTENT, "change {value} lands");
    }

    let first = first_attempt(&server).await;
    let dormancy = evict_until_dormant(&client, &broker, &tenant, &server).await;
    let after = attempts_after_dormancy(&server, first, &dormancy).await;
    server.shutdown().await.expect("shutdown");
    assert_retried_after_dormancy("a busy outbox", &dormancy, after);
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

/// Provision one tenant on `broker`, touch it so its runtime and pool are
/// resident, and return its name and database.
async fn resident_tenant(client: &Client, pool: &PgPool, broker: &str, prefix: &str) -> (String, TenantDatabaseName) {
    let tenant = tenant_load::tenant_names(prefix, 1).remove(0);
    tenant_load::provision_tenants(client, broker, slice::from_ref(&tenant), CORE_CONTEXT, 1)
        .await
        .expect("every tenant is provisioned");
    let id = urlencoding::encode(&format!("urn:ngsi-ld:Device:provision:{tenant}")).into_owned();
    let touched = client
        .get(format!("{broker}entities/{id}"))
        .header("NGSILD-Tenant", tenant.as_str())
        .send()
        .await
        .expect("touch the tenant")
        .status();
    assert!(touched.is_success(), "the provisioned tenant answers, got {touched}");
    let database = tenant_database(pool, &tenant).await;
    (tenant, database)
}

/// The pooled broker's client connections to `database` that `PgBouncer` holds,
/// in any state -- idle, linked to a server or queued for one.
///
/// Attributed by `application_name`, because every broker on the tier shares the
/// tenant registry and another broker's wake probe may reach the same database.
async fn pooler_clients(console: &PoolerConsole, database: &TenantDatabaseName) -> usize {
    console
        .show_clients()
        .await
        .expect("read SHOW CLIENTS")
        .iter()
        .filter(|row| row.database == database.as_str() && row.application_name == endpoints::BROKER_APPLICATION_NAME)
        .count()
}

#[tokio::test]
#[serial_test::serial]
async fn suspending_a_pooled_tenant_disconnects_its_pooler_clients_before_answering() {
    // A suspend releases the tenant's runtime synchronously: the 204 is sent only
    // after its pool closed (docs/tenant-management.md, "Suspend and resume").
    // Behind the pooler the broker's connections are PgBouncer *client*
    // connections, so that is what must be gone the moment the suspend answers.
    // The PostgreSQL backends behind them belong to PgBouncer and linger until its
    // server_idle_timeout, which says nothing about the broker, so they are not
    // what this asserts.
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let console = PoolerConsole::new(endpoints::pgbouncer_admin_url());
    let (tenant, database) = resident_tenant(&client, &pool, &broker, "pgbsuspend").await;
    let before = pooler_clients(&console, &database).await;
    assert!(before > 0, "a resident pooled tenant holds pooler client connections, saw {before}");

    suspend(&client, &broker, &tenant, false).await;

    let after = pooler_clients(&console, &database).await;
    assert_eq!(after, 0, "a suspend disconnects the tenant's pooler clients before it answers, saw {after}");

    reset(&client, &pool).await;
    pool.close().await;
}

#[tokio::test]
#[serial_test::serial]
async fn suspending_an_unpooled_tenant_closes_its_backends() {
    // Without a pooler the broker's connections are the PostgreSQL backends
    // themselves. The suspend closes them before it answers; a backend then exits
    // on the server asynchronously to the client's close, so its row may linger
    // in pg_stat_activity for the moment the server takes to reap it -- bounded
    // here, and far inside any janitor interval, which would not evict a
    // suspended-but-registered tenant anyway.
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker_direct();
    let (tenant, database) = resident_tenant(&client, &pool, &broker, "pgbsuspdirect").await;
    let direct = [endpoints::BROKER_DIRECT_APPLICATION_NAME];
    let before = backend_sampler::client_backends_of(&pool, database.as_str(), &direct)
        .await
        .expect("count backends");
    assert!(before > 0, "a resident un-pooled tenant holds backends, saw {before}");

    suspend(&client, &broker, &tenant, false).await;

    let mut after = backend_sampler::client_backends_of(&pool, database.as_str(), &direct)
        .await
        .expect("count backends");
    let deadline = Instant::now() + Duration::from_secs(2);
    while after > 0 && Instant::now() < deadline {
        sleep(Duration::from_millis(50)).await;
        after = backend_sampler::client_backends_of(&pool, database.as_str(), &direct)
            .await
            .expect("count backends");
    }
    assert_eq!(after, 0, "a suspend closes the un-pooled tenant's backends, saw {after}");

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
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1)
        .await
        .expect("every tenant is provisioned");

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
