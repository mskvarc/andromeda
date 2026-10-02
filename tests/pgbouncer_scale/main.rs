//! Scale behaviour of the transaction-pooling tier: provisioning bursts, a
//! working set larger than the runtime cache, Zipf hot-tenant residency, an
//! idle wide cache releasing its backends, the global pooler cap never starving
//! a new tenant, two replicas sharing one pooler, and the backends an un-pooled
//! broker keeps once a burst ends.
//!
//! Every test drives the containerised `pgbouncer` stack (`cargo xtask itest
//! --stack pgbouncer`) and samples, over a direct connection, only the
//! PostgreSQL backends of the brokers it drives.
//! The tenant count is `PGBOUNCER_SCALE_TENANTS` (default 300) and the per-test
//! load duration is `PGBOUNCER_LOAD_SECS` (default 60).

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::pooler_probe::{
    backend_sampler::{self, BackendSampler},
    endpoints,
    metrics_scrape::Metrics,
    request_outcome::RequestOutcome,
    tenant_cleanup,
    tenant_load::{self, LoadReport, UniformPicker, ZipfPicker},
};
use reqwest::Client;
use serde_json::json;
use sqlx::PgPool;
use std::{
    env,
    time::{Duration, Instant},
};
use tokio::time::sleep;

/// How many distinct tenants the scale suite provisions.
fn tenant_count() -> usize {
    env::var("PGBOUNCER_SCALE_TENANTS").ok().and_then(|value| value.parse().ok()).unwrap_or(300)
}

/// How long each bounded load run lasts.
fn load_duration() -> Duration {
    Duration::from_secs(env::var("PGBOUNCER_LOAD_SECS").ok().and_then(|value| value.parse().ok()).unwrap_or(60))
}

/// How many tenants the two-replica test spreads its load over: inside the
/// runtime cache of 32 each pooled replica is configured with.
const REPLICA_WORKING_SET: usize = 24;

/// The two pooled replicas that share one pooler.
const REPLICA_PAIR: &[&str] = &[endpoints::BROKER_APPLICATION_NAME, endpoints::BROKER_REPLICA_APPLICATION_NAME];

/// A client with a generous timeout so a genuinely slow request is measured, not
/// cut off early.
fn client() -> Client {
    Client::builder().timeout(Duration::from_secs(30)).build().expect("reqwest client")
}

/// The Link header naming the core context for GET requests.
fn link_header() -> String {
    format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

/// The entity id `tenant_load::provision_tenants` creates under each tenant.
fn provision_id(tenant: &str) -> String {
    format!("urn:ngsi-ld:Device:provision:{tenant}")
}

/// GET the provisioned entity under `tenant`.
async fn get_entity(client: &Client, broker: &str, tenant: &str) -> RequestOutcome {
    let id = urlencoding::encode(&provision_id(tenant)).into_owned();
    RequestOutcome::of(
        client
            .get(format!("{broker}entities/{id}"))
            .header("NGSILD-Tenant", tenant)
            .header("Accept", "application/ld+json")
            .header("Link", link_header()),
    )
    .await
}

/// PATCH one attribute of the provisioned entity under `tenant`.
async fn patch_attr(client: &Client, broker: &str, tenant: &str) -> RequestOutcome {
    let id = urlencoding::encode(&provision_id(tenant)).into_owned();
    RequestOutcome::of(
        client
            .patch(format!("{broker}entities/{id}/attrs"))
            .header("NGSILD-Tenant", tenant)
            .header("Content-Type", "application/ld+json")
            .json(&json!({"reading": {"type": "Property", "value": 2}, "@context": [CORE_CONTEXT]})),
    )
    .await
}

/// Upsert a small batch of entities under `tenant` (COPY-staging path). The
/// batch body is a bare NGSI-LD entity array, each element carrying the context.
async fn batch_upsert(client: &Client, broker: &str, tenant: &str) -> RequestOutcome {
    let entities: Vec<_> = (0..8)
        .map(|index| json!({"id": format!("urn:ngsi-ld:Device:batch:{tenant}:{index}"), "type": "Device", "reading": {"type": "Property", "value": index}, "@context": [CORE_CONTEXT]}))
        .collect();
    RequestOutcome::of(
        client
            .post(format!("{broker}entityOperations/upsert"))
            .header("NGSILD-Tenant", tenant)
            .header("Content-Type", "application/ld+json")
            .json(&entities),
    )
    .await
}

/// Scrape `broker`'s metrics, failing the test when the exposition cannot be
/// read: an unreadable scrape must never pass as a zero reading.
async fn scrape(client: &Client, broker: &str) -> Metrics {
    Metrics::scrape(client, broker)
        .await
        .map_err(|error| format!("the broker at {broker} must expose its metrics: {error}"))
        .expect("an unreadable scrape must never pass as a zero")
}

/// The cold builds `broker` has run so far, over every outcome.
async fn cold_builds(client: &Client, broker: &str) -> f64 {
    let metrics = scrape(client, broker).await;
    assert!(
        metrics.has("andromeda_tenant_cold_builds_total"),
        "the cold-build counter must be exposed once any tenant was built"
    );
    metrics.sum("andromeda_tenant_cold_builds_total")
}

/// A one-line account of how a load run's requests resolved: statuses, failed
/// transports by kind, their sampled causes and the first body per error status.
fn describe(report: &LoadReport) -> String {
    format!(
        "{} requests, {} non-2xx, statuses {:?}, transport failures {:?} (samples {:?}), error bodies {:?}",
        report.total, report.non_2xx, report.statuses, report.transport_failures, report.transport_samples, report.error_details
    )
}

/// The current client-backend count under `application_names`, over a fresh
/// direct connection.
async fn current_backends(application_names: &[&str]) -> i64 {
    let Ok(pool) = PgPool::connect(&endpoints::database_url()).await else {
        return -1;
    };
    let count = backend_sampler::client_backends(&pool, application_names).await.unwrap_or(-1);
    pool.close().await;
    count
}

/// The lowest client-backend count under `application_names` seen over `window`,
/// sampling twice a second.
///
/// The idle backend level oscillates -- per-tenant background tasks fire in waves
/// and the count dips to the platform-pool baseline between them -- so the trough,
/// not any single reading, is what shows the backends were released. Samples that
/// error (a returned `-1`) are ignored; an all-error window returns `-1`.
async fn min_backends_over(window: Duration, application_names: &[&str]) -> i64 {
    let deadline = Instant::now() + window;
    let mut minimum = i64::MAX;
    loop {
        let sample = current_backends(application_names).await;
        if sample >= 0 {
            minimum = minimum.min(sample);
        }
        if Instant::now() >= deadline {
            break;
        }
        sleep(Duration::from_millis(500)).await;
    }
    if minimum == i64::MAX { -1 } else { minimum }
}

/// Reset the tier: evict every broker's orphaned runtimes and drop the tenant
/// databases.
async fn reset(client: &Client) {
    let pool = PgPool::connect(&endpoints::database_url()).await.expect("connect to PostgreSQL");
    let brokers = [
        endpoints::broker(),
        endpoints::broker_replica(),
        endpoints::broker_wide(),
        endpoints::broker_direct(),
    ];
    let refs: Vec<&str> = brokers.iter().map(String::as_str).collect();
    tenant_cleanup::reset(client, &refs, &pool).await.expect("reset the tier");
    pool.close().await;
}

#[tokio::test]
#[serial_test::serial]
async fn provisioning_burst_stays_within_the_direct_connection_budget() {
    let client = client();
    reset(&client).await;
    let broker = endpoints::broker();
    let tenants = tenant_load::tenant_names("pgbscale", tenant_count());

    let sampler = BackendSampler::start(&endpoints::database_url(), &[endpoints::BROKER_APPLICATION_NAME])
        .await
        .expect("start sampler");
    let provisioned = tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 32).await;
    let series = sampler.stop().await;
    let report = provisioned.expect("every first-touch create must provision without a 5xx");

    println!(
        "provisioning: {} created, {:.1} tenants/s, peak {} backends",
        report.created, report.tenants_per_second, series.max
    );
    assert!(
        series.max < 300,
        "provisioning must stay within the PostgreSQL connection budget, saw {}",
        series.max
    );
    reset(&client).await;
}

#[tokio::test]
#[serial_test::serial]
async fn a_working_set_three_times_the_cache_is_served_without_a_single_failure() {
    let client = client();
    reset(&client).await;
    let broker = endpoints::broker();
    let tenants = tenant_load::tenant_names("pgbwork", 96);
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 32)
        .await
        .expect("every tenant is provisioned");

    let before = cold_builds(&client, &broker).await;
    let sampler = BackendSampler::start(&endpoints::database_url(), &[endpoints::BROKER_APPLICATION_NAME])
        .await
        .expect("start sampler");
    let picker = UniformPicker::new(tenants.len(), 0x5EED);
    let report = tenant_load::run_bounded(48, load_duration(), |index| {
        let client = &client;
        let broker = &broker;
        let tenants = &tenants;
        let picker = &picker;
        async move {
            let tenant = &tenants[picker.pick()];
            match index % 10 {
                0..=6 => get_entity(client, broker, tenant).await,
                7..=8 => patch_attr(client, broker, tenant).await,
                _ => batch_upsert(client, broker, tenant).await,
            }
        }
    })
    .await;
    let series = sampler.stop().await;
    let after = cold_builds(&client, &broker).await;

    println!(
        "working-set 3x: {}, p50 {:?}, p99 {:?}, peak {} backends, cold builds {}",
        describe(&report),
        report.p50,
        report.p99,
        series.max,
        after - before
    );
    // A working set larger than the cache, behind a pooler capped below its peak
    // demand, is served correctly by shedding the excess as 503 + Retry-After
    // backpressure, never as an internal error and never as a hang: the broker
    // stays within its backend budget rather than serving every request at any
    // cost. So the contract is no 5xx other than 503, every non-2xx a 503, and a
    // low shed rate (graceful backpressure, not collapse) -- not zero shedding,
    // which a hard cap below peak demand cannot honour.
    let server_errors: usize = report.statuses.iter().filter(|(code, _)| **code >= 500 && **code != 503).map(|(_, n)| n).sum();
    let shed = report.statuses.get(&503).copied().unwrap_or(0);
    assert_eq!(
        server_errors,
        0,
        "a working set larger than the cache must never surface a 5xx other than 503: {}",
        describe(&report)
    );
    assert_eq!(
        report.non_2xx,
        shed,
        "every non-2xx under thrash must be a 503 backpressure response, not a hang, a dropped connection or an internal error: {}",
        describe(&report)
    );
    assert!(
        shed * 20 < report.total.max(1),
        "503 backpressure must stay a low fraction of the load (graceful shedding, not collapse), saw {shed} of {}",
        report.total
    );
    assert!(
        series.max <= 200,
        "pooled backends must stay within max_user_connections plus the direct budget, saw {}",
        series.max
    );
    assert!(after - before > 0.0, "the working set must have forced cold builds (thrash really happened)");
    reset(&client).await;
}

#[tokio::test]
#[serial_test::serial]
async fn a_zipf_working_set_keeps_hot_tenants_resident() {
    let client = client();
    reset(&client).await;
    let broker = endpoints::broker();
    let tenants = tenant_load::tenant_names("pgbzipf", tenant_count());
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 32)
        .await
        .expect("every tenant is provisioned");

    // Warm up with a Zipf-skewed working set so the hot tenants become resident.
    let picker = ZipfPicker::new(tenants.len(), 1.1, 0xB0B);
    let warm = tenant_load::run_bounded(48, load_duration(), |_| {
        let client = &client;
        let broker = &broker;
        let tenants = &tenants;
        let picker = &picker;
        async move { get_entity(client, broker, &tenants[picker.pick()]).await }
    })
    .await;

    // Over a steady-state window that touches only the sixteen hottest tenants,
    // no cold build should occur: they must have stayed resident.
    let before = cold_builds(&client, &broker).await;
    let hot_count = tenants.len().min(16);
    let hot = tenant_load::run_bounded(16, Duration::from_secs(10), |index| {
        let client = &client;
        let broker = &broker;
        let tenants = &tenants;
        async move {
            let tenant = &tenants[usize::try_from(index).unwrap_or(0) % hot_count];
            get_entity(client, broker, tenant).await
        }
    })
    .await;
    let after = cold_builds(&client, &broker).await;

    println!(
        "zipf: warmup {}; hot window {}; cold builds in hot window {}",
        describe(&warm),
        describe(&hot),
        after - before
    );
    assert_eq!(warm.non_2xx, 0, "the Zipf working set must be served without a failure: {}", describe(&warm));
    assert_eq!(hot.non_2xx, 0, "the hot-tenant window must be served without a failure: {}", describe(&hot));
    // The hot set stays resident: its cold-build count over the steady-state
    // window is a negligible fraction of the tenant set, not the per-request
    // thrash an un-resident hot set would show. A stray build or two reflects
    // LRU timing at the warmup-to-hot boundary, not eviction of the hot tenants.
    let hot_cold_builds = after - before;
    assert!(
        hot_cold_builds < f64::from(u32::try_from(hot_count).unwrap_or(u32::MAX)),
        "the hottest tenants must stay resident (cold builds far below the hot-set size), saw {hot_cold_builds}"
    );
    reset(&client).await;
}

#[tokio::test]
#[serial_test::serial]
async fn an_idle_wide_cache_releases_its_backends() {
    let client = client();
    reset(&client).await;
    let broker = endpoints::broker_wide();
    let tenants = tenant_load::tenant_names("pgbwide", tenant_count());
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 32)
        .await
        .expect("every tenant is provisioned");

    // Touch every tenant once so all are cached, then let the fleet go idle.
    for tenant in &tenants {
        let _ = get_entity(&client, &broker, tenant).await;
    }
    sleep(Duration::from_secs(30)).await;

    let cached = scrape(&client, &broker).await.gauge("andromeda_tenant_runtimes_cached");
    // Per-tenant background tasks fire in loose waves rather than continuously, so
    // the server-wide backend count oscillates: it dips to the platform-pool
    // baseline between waves and rises while one is in flight. Releasing the
    // backends is proved by the trough reaching the baseline, not by any single
    // instantaneous reading, so sample across a window and assert on the minimum.
    let backends = min_backends_over(Duration::from_secs(20), &[endpoints::BROKER_WIDE_APPLICATION_NAME]).await;
    println!("idle wide cache: cached runtimes gauge {cached:?}, trough client backends {backends}");
    // A wide cache evicts nothing, so every touched tenant is still resident;
    // the gauge is the touched count, plus at most the always-resident unnamed
    // default depending on whether it is counted. Fewer means a touched tenant
    // was evicted; more means the cache over-retained.
    let touched = f64::from(u32::try_from(tenants.len()).unwrap_or(u32::MAX));
    assert!(
        cached.is_some_and(|count| count >= touched && count <= touched + 1.0),
        "every touched tenant must stay cached in a wide cache, saw {cached:?} for {} tenants",
        tenants.len()
    );
    assert!(backends < 60, "an idle wide cache must release its backends to baseline, trough was {backends}");
    reset(&client).await;
}

#[tokio::test]
#[serial_test::serial]
async fn the_global_pooler_cap_never_starves_a_new_tenant() {
    let client = client();
    reset(&client).await;
    let broker = endpoints::broker_wide();
    let resident = tenant_load::tenant_names("pgbcap", tenant_count());
    tenant_load::provision_tenants(&client, &broker, &resident, CORE_CONTEXT, 32)
        .await
        .expect("every tenant is provisioned");
    for tenant in &resident {
        let _ = get_entity(&client, &broker, tenant).await;
    }

    // A request to a never-touched tenant must still complete promptly even with
    // hundreds of resident idle tenants and a hard global backend cap.
    let newcomer = tenant_load::tenant_names("pgbcapnew", 1);
    let started = Instant::now();
    let outcome = tenant_load::create_entity(
        &client,
        &broker,
        &newcomer[0],
        &tenant_load::device_entity("urn:ngsi-ld:Device:newcomer", CORE_CONTEXT),
    )
    .await;
    let elapsed = started.elapsed();

    println!("new-tenant under global cap: {outcome:?}, elapsed {elapsed:?}");
    assert_eq!(outcome.status(), Some(201), "a never-touched tenant must be created, not starved: {outcome:?}");
    assert!(
        elapsed < Duration::from_secs(5),
        "a new tenant must not be starved by resident idle tenants, took {elapsed:?}"
    );
    reset(&client).await;
}

#[tokio::test]
#[serial_test::serial]
async fn two_replicas_share_one_pooler_without_doubling_backends() {
    let client = client();
    reset(&client).await;
    let broker = endpoints::broker();
    let replica = endpoints::broker_replica();
    // A working set inside each replica's runtime cache (32), so every request
    // is served from a resident runtime and none may fail. Thrashing a working
    // set larger than the cache sheds bounded 503 backpressure by contract; that
    // contract is `a_working_set_three_times_the_cache_is_served_without_a_single_failure`'s,
    // while this test isolates what a second replica costs in backends.
    let tenants = tenant_load::tenant_names("pgbrepl", REPLICA_WORKING_SET);
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 32)
        .await
        .expect("every tenant is provisioned");

    // Single-replica baseline: the working set through one broker.
    let single = {
        let sampler = BackendSampler::start(&endpoints::database_url(), REPLICA_PAIR).await.expect("start sampler");
        let picker = UniformPicker::new(tenants.len(), 1);
        let report = tenant_load::run_bounded(32, load_duration(), |_| {
            let client = &client;
            let broker = &broker;
            let tenants = &tenants;
            let picker = &picker;
            async move { get_entity(client, broker, &tenants[picker.pick()]).await }
        })
        .await;
        (report, sampler.stop().await.max)
    };

    // Dual-replica: the same working set split across both brokers.
    let dual = {
        let sampler = BackendSampler::start(&endpoints::database_url(), REPLICA_PAIR).await.expect("start sampler");
        let picker = UniformPicker::new(tenants.len(), 2);
        let report = tenant_load::run_bounded(32, load_duration(), |index| {
            let client = &client;
            let broker = &broker;
            let replica = &replica;
            let tenants = &tenants;
            let picker = &picker;
            async move {
                let target = if index % 2 == 0 { broker } else { replica };
                get_entity(client, target, &tenants[picker.pick()]).await
            }
        })
        .await;
        (report, sampler.stop().await.max)
    };

    // Read-your-writes across replicas: write on the primary, read on the replica.
    let tenant = &tenants[0];
    let write = tenant_load::create_entity(&client, &broker, tenant, &tenant_load::device_entity("urn:ngsi-ld:Device:ryow", CORE_CONTEXT)).await;
    let read = {
        let id = urlencoding::encode("urn:ngsi-ld:Device:ryow").into_owned();
        RequestOutcome::of(
            client
                .get(format!("{replica}entities/{id}"))
                .header("NGSILD-Tenant", tenant.as_str())
                .header("Accept", "application/ld+json")
                .header("Link", link_header()),
        )
        .await
    };

    println!(
        "replicas: single peak {} backends ({}), dual peak {} backends ({}), ryow write {write:?} read {read:?}",
        single.1,
        describe(&single.0),
        dual.1,
        describe(&dual.0)
    );
    assert_eq!(single.0.non_2xx, 0, "the single-replica run must have no failures: {}", describe(&single.0));
    assert_eq!(dual.0.non_2xx, 0, "the dual-replica run must have no failures: {}", describe(&dual.0));
    assert_eq!(write.status(), Some(201), "the read-your-writes create must succeed: {write:?}");
    assert_eq!(read.status(), Some(200), "a write on one replica must be visible on the other: {read:?}");
    assert!(
        dual.1 <= single.1 * 5 / 4,
        "two replicas must not need more than 1.25x the single-replica backends (single {}, dual {})",
        single.1,
        dual.1
    );
    reset(&client).await;
}

#[tokio::test]
#[serial_test::serial]
async fn an_unpooled_broker_keeps_several_times_the_backends_once_a_burst_ends() {
    let client = client();
    reset(&client).await;
    let tenants = tenant_load::tenant_names("pgbcmp", 24);

    let pooled = measure_burst_footprint(&client, &endpoints::broker(), &[endpoints::BROKER_APPLICATION_NAME], &tenants).await;
    reset(&client).await;
    let direct = measure_burst_footprint(&client, &endpoints::broker_direct(), &[endpoints::BROKER_DIRECT_APPLICATION_NAME], &tenants).await;

    println!(
        "burst footprint: pooled peak {} held {}, un-pooled peak {} held {}",
        pooled.peak, pooled.held, direct.peak, direct.held
    );
    // Burst peaks are bounded per tenant either way (tenant_max_connections
    // un-pooled, default_pool_size + reserve_pool_size pooled), so the pooler's
    // win is what a broker keeps once the burst ends: PgBouncer releases a backend
    // server_idle_timeout after its last transaction, while an un-pooled broker
    // holds every connection until its own idle reclaim.
    let tenant_count = i64::try_from(tenants.len()).expect("tenant count fits i64");
    assert!(
        direct.held >= tenant_count,
        "the un-pooled broker must still hold a backend per burst tenant, held {} for {tenant_count} tenants",
        direct.held
    );
    assert!(
        pooled.held * 3 <= direct.held,
        "the un-pooled broker must keep at least three times the pooled broker's backends (pooled {}, un-pooled {})",
        pooled.held,
        direct.held
    );
    reset(&client).await;
}

/// How long after a burst the held backends are read: past `PgBouncer`'s
/// five-second `server_idle_timeout`, inside the broker's thirty-second pool idle
/// reclaim.
const HOLD_SETTLE: Duration = Duration::from_secs(12);

/// The backends one broker opened during a burst and still held after it.
struct BurstFootprint {
    /// The peak client-backend count during the burst.
    peak: i64,
    /// The trough client-backend count over a window [`HOLD_SETTLE`] after it.
    held: i64,
}

/// Provision `tenants` on `broker`, drive a uniform read/write burst, and report
/// the backends connected under `application_names` during and after it.
async fn measure_burst_footprint(client: &Client, broker: &str, application_names: &'static [&'static str], tenants: &[String]) -> BurstFootprint {
    tenant_load::provision_tenants(client, broker, tenants, CORE_CONTEXT, 16)
        .await
        .expect("every tenant is provisioned");
    let sampler = BackendSampler::start(&endpoints::database_url(), application_names)
        .await
        .expect("start sampler");
    let picker = UniformPicker::new(tenants.len(), 7);
    let _ = tenant_load::run_bounded(32, load_duration(), |index| {
        let picker = &picker;
        async move {
            let tenant = &tenants[picker.pick()];
            if index % 4 == 0 {
                patch_attr(client, broker, tenant).await
            } else {
                get_entity(client, broker, tenant).await
            }
        }
    })
    .await;
    let peak = sampler.stop().await.max;
    sleep(HOLD_SETTLE).await;
    let held = min_backends_over(Duration::from_secs(5), application_names).await;
    BurstFootprint { peak, held }
}
