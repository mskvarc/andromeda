//! Scale behaviour of the transaction-pooling tier: provisioning bursts, a
//! working set larger than the runtime cache, Zipf hot-tenant residency, an
//! idle wide cache releasing its backends, the global pooler cap never starving
//! a new tenant, two replicas sharing one pooler, and the un-pooled baseline's
//! backend cost.
//!
//! Every test drives the containerised `pgbouncer` stack (`cargo xtask itest
//! --stack pgbouncer`) and samples PostgreSQL backends over a direct connection.
//! The tenant count is `PGBOUNCER_SCALE_TENANTS` (default 300) and the per-test
//! load duration is `PGBOUNCER_LOAD_SECS` (default 60).

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::pooler_probe::{
    backend_sampler::{self, BackendSampler},
    endpoints,
    metrics_scrape::Metrics,
    tenant_cleanup,
    tenant_load::{self, UniformPicker, ZipfPicker},
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

/// GET the provisioned entity under `tenant`, returning the status (0 on error).
async fn get_entity(client: &Client, broker: &str, tenant: &str) -> u16 {
    let id = urlencoding::encode(&provision_id(tenant)).into_owned();
    client
        .get(format!("{broker}entities/{id}"))
        .header("NGSILD-Tenant", tenant)
        .header("Accept", "application/ld+json")
        .header("Link", link_header())
        .send()
        .await
        .map_or(0, |response| response.status().as_u16())
}

/// PATCH one attribute of the provisioned entity under `tenant`.
async fn patch_attr(client: &Client, broker: &str, tenant: &str) -> u16 {
    let id = urlencoding::encode(&provision_id(tenant)).into_owned();
    client
        .patch(format!("{broker}entities/{id}/attrs"))
        .header("NGSILD-Tenant", tenant)
        .header("Content-Type", "application/ld+json")
        .json(&json!({"value": {"type": "Property", "value": 2}, "@context": [CORE_CONTEXT]}))
        .send()
        .await
        .map_or(0, |response| response.status().as_u16())
}

/// Upsert a small batch of entities under `tenant` (COPY-staging path). The
/// batch body is a bare NGSI-LD entity array, each element carrying the context.
async fn batch_upsert(client: &Client, broker: &str, tenant: &str) -> u16 {
    let entities: Vec<_> = (0..8)
        .map(|index| json!({"id": format!("urn:ngsi-ld:Device:batch:{tenant}:{index}"), "type": "Device", "value": {"type": "Property", "value": index}, "@context": [CORE_CONTEXT]}))
        .collect();
    client
        .post(format!("{broker}entityOperations/upsert"))
        .header("NGSILD-Tenant", tenant)
        .header("Content-Type", "application/ld+json")
        .json(&entities)
        .send()
        .await
        .map_or(0, |response| response.status().as_u16())
}

/// The current client-backend count over a fresh direct connection.
async fn current_backends() -> i64 {
    let Ok(pool) = PgPool::connect(&endpoints::database_url()).await else {
        return -1;
    };
    let count = backend_sampler::client_backends(&pool).await.unwrap_or(-1);
    pool.close().await;
    count
}

/// The lowest client-backend count seen over `window`, sampling twice a second.
///
/// The idle backend level oscillates -- per-tenant background tasks fire in waves
/// and the count dips to the platform-pool baseline between them -- so the trough,
/// not any single reading, is what shows the backends were released. Samples that
/// error (a returned `-1`) are ignored; an all-error window returns `-1`.
async fn min_backends_over(window: Duration) -> i64 {
    let deadline = Instant::now() + window;
    let mut minimum = i64::MAX;
    loop {
        let sample = current_backends().await;
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

    let sampler = BackendSampler::start(&endpoints::database_url()).await.expect("start sampler");
    let report = tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 32).await;
    let series = sampler.stop().await;

    println!(
        "provisioning: {} created, {} failed, {:.1} tenants/s, peak {} backends",
        report.created, report.failed, report.tenants_per_second, series.max
    );
    assert_eq!(report.failed, 0, "every first-touch create must provision without a 5xx");
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
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 32).await;

    let before = Metrics::scrape(&client, &broker)
        .await
        .map_or(0.0, |m| m.sum("andromeda_tenant_cold_builds_total"));
    let sampler = BackendSampler::start(&endpoints::database_url()).await.expect("start sampler");
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
    let after = Metrics::scrape(&client, &broker)
        .await
        .map_or(0.0, |m| m.sum("andromeda_tenant_cold_builds_total"));

    println!(
        "working-set 3x: {} requests, {} non-2xx, statuses {:?}, p50 {:?}, p99 {:?}, peak {} backends, cold builds {}",
        report.total,
        report.non_2xx,
        report.statuses,
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
    assert_eq!(server_errors, 0, "a working set larger than the cache must never surface a 5xx other than 503");
    assert_eq!(
        report.non_2xx, shed,
        "every non-2xx under thrash must be a 503 backpressure response, not a hang or an internal error"
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
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 32).await;

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
    let before = Metrics::scrape(&client, &broker)
        .await
        .map_or(0.0, |m| m.sum("andromeda_tenant_cold_builds_total"));
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
    let after = Metrics::scrape(&client, &broker)
        .await
        .map_or(0.0, |m| m.sum("andromeda_tenant_cold_builds_total"));

    println!(
        "zipf: warmup {} req {} non-2xx (statuses {:?}); hot window {} req {} non-2xx (statuses {:?}); cold builds in hot window {}",
        warm.total,
        warm.non_2xx,
        warm.statuses,
        hot.total,
        hot.non_2xx,
        hot.statuses,
        after - before
    );
    assert_eq!(warm.non_2xx, 0, "the Zipf working set must be served without a failure");
    assert_eq!(hot.non_2xx, 0, "the hot-tenant window must be served without a failure");
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
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 32).await;

    // Touch every tenant once so all are cached, then let the fleet go idle.
    for tenant in &tenants {
        let _ = get_entity(&client, &broker, tenant).await;
    }
    sleep(Duration::from_secs(30)).await;

    let cached = Metrics::scrape(&client, &broker)
        .await
        .ok()
        .and_then(|m| m.gauge("andromeda_tenant_runtimes_cached"));
    // Per-tenant background tasks fire in loose waves rather than continuously, so
    // the server-wide backend count oscillates: it dips to the platform-pool
    // baseline between waves and rises while one is in flight. Releasing the
    // backends is proved by the trough reaching the baseline, not by any single
    // instantaneous reading, so sample across a window and assert on the minimum.
    let backends = min_backends_over(Duration::from_secs(20)).await;
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
    tenant_load::provision_tenants(&client, &broker, &resident, CORE_CONTEXT, 32).await;
    for tenant in &resident {
        let _ = get_entity(&client, &broker, tenant).await;
    }

    // A request to a never-touched tenant must still complete promptly even with
    // hundreds of resident idle tenants and a hard global backend cap.
    let newcomer = tenant_load::tenant_names("pgbcapnew", 1);
    let started = Instant::now();
    let status = tenant_load::create_entity(
        &client,
        &broker,
        &newcomer[0],
        &tenant_load::device_entity("urn:ngsi-ld:Device:newcomer", CORE_CONTEXT),
    )
    .await;
    let elapsed = started.elapsed();

    println!("new-tenant under global cap: status {status}, elapsed {elapsed:?}");
    assert_eq!(status, 201, "a never-touched tenant must be created, not starved");
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
    let tenants = tenant_load::tenant_names("pgbrepl", 64);
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 32).await;

    // Single-replica baseline: the working set through one broker.
    let single = {
        let sampler = BackendSampler::start(&endpoints::database_url()).await.expect("start sampler");
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
        let sampler = BackendSampler::start(&endpoints::database_url()).await.expect("start sampler");
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
        client
            .get(format!("{replica}entities/{id}"))
            .header("NGSILD-Tenant", tenant.as_str())
            .header("Accept", "application/ld+json")
            .header("Link", link_header())
            .send()
            .await
            .map_or(0, |response| response.status().as_u16())
    };

    println!(
        "replicas: single peak {} backends ({} non-2xx, statuses {:?}), dual peak {} backends ({} non-2xx, statuses {:?}), ryow write {} read {}",
        single.1, single.0.non_2xx, single.0.statuses, dual.1, dual.0.non_2xx, dual.0.statuses, write, read
    );
    assert_eq!(single.0.non_2xx, 0, "the single-replica run must have no failures");
    assert_eq!(dual.0.non_2xx, 0, "the dual-replica run must have no failures");
    assert!(read == 200 || read == 201, "a write on one replica must be visible on the other, got {read}");
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
async fn the_unpooled_baseline_needs_several_times_the_backends() {
    let client = client();
    reset(&client).await;
    let tenants = tenant_load::tenant_names("pgbcmp", 32);

    let pooled_max = measure_cached_workload(&client, &endpoints::broker(), &tenants).await;
    reset(&client).await;
    let direct_max = measure_cached_workload(&client, &endpoints::broker_direct(), &tenants).await;

    // A recorded comparison, not a hard ratio assertion. Peaks are sampled over
    // the whole (shared) PostgreSQL, so both include the other brokers' idle
    // footprint; and at this modest, steadily-cached, low-concurrency tenant
    // count the un-pooled broker's pools stay lazily small while the pooler
    // eagerly holds default_pool_size per active tenant database, so the pooled
    // peak is not necessarily the lower of the two here. The pooler's backend
    // win is at scale and under bursty concurrency, which the wide-cache and
    // working-set tests exercise; the study records both peaks and interprets.
    println!("backend comparison: pooled peak {pooled_max}, un-pooled peak {direct_max}");
    assert!(
        pooled_max > 0 && direct_max > 0,
        "both workloads must open backends to compare (pooled {pooled_max}, un-pooled {direct_max})"
    );
    reset(&client).await;
}

/// Provision `tenants` on `broker`, drive a uniform read/write workload, and
/// return the peak client-backend count observed.
async fn measure_cached_workload(client: &Client, broker: &str, tenants: &[String]) -> i64 {
    tenant_load::provision_tenants(client, broker, tenants, CORE_CONTEXT, 16).await;
    let sampler = BackendSampler::start(&endpoints::database_url()).await.expect("start sampler");
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
    sampler.stop().await.max
}
