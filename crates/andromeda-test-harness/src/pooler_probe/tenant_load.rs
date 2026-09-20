//! Provisioning tenant sets and generating bounded load against a broker.
//!
//! Provides the tenant-name generation, the uniform and Zipf tenant pickers
//! (over a small deterministic PRNG so a run is reproducible), and the bounded
//! concurrent runner that records per-request latency and status so a test can
//! assert on failure counts and report p50/p99.

use futures::{
    future::join_all,
    stream::{self, StreamExt},
};
use reqwest::Client;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use uuid::Uuid;

/// A set of distinct tenant names sharing a prefix, each suffixed with a fresh
/// UUID so runs never collide.
#[must_use]
pub fn tenant_names(prefix: &str, count: usize) -> Vec<String> {
    (0..count).map(|_| format!("{prefix}{}", Uuid::new_v4().simple())).collect()
}

/// A minimal `Device` entity carrying one integer property, under `context`.
#[must_use]
pub fn device_entity(id: &str, context: &str) -> Value {
    json!({
        "id": id,
        "type": "Device",
        "value": {"type": "Property", "value": 1},
        "@context": [context]
    })
}

/// The outcome of provisioning a tenant set.
#[derive(Debug, Clone)]
pub struct ProvisionReport {
    /// How many creates returned 201.
    pub created: usize,
    /// How many creates returned anything other than 201.
    pub failed: usize,
    /// Wall-clock time the whole burst took.
    pub elapsed: Duration,
    /// Provisioning throughput in tenants per second.
    pub tenants_per_second: f64,
    /// Every response status, transport failures recorded as 0.
    pub statuses: Vec<u16>,
}

/// Provision every tenant by creating one `Device` entity under it, at most
/// `concurrency` creates in flight, and report the outcome.
///
/// A first-touch create under a fresh tenant provisions its database, so this
/// both materialises the tenant set and measures the provisioning burst.
pub async fn provision_tenants(client: &Client, broker_base: &str, tenants: &[String], context: &str, concurrency: usize) -> ProvisionReport {
    let started = Instant::now();
    let requests = tenants.iter().cloned().map(|tenant| async move {
        let id = format!("urn:ngsi-ld:Device:provision:{tenant}");
        create_entity(client, broker_base, &tenant, &device_entity(&id, context)).await
    });
    let statuses: Vec<u16> = stream::iter(requests).buffered(concurrency.max(1)).collect().await;
    let elapsed = started.elapsed();
    let created = statuses.iter().filter(|status| **status == 201).count();
    let seconds = elapsed.as_secs_f64();
    let tenants_per_second = if seconds > 0.0 { as_f64(tenants.len()) / seconds } else { 0.0 };
    ProvisionReport {
        created,
        failed: statuses.len() - created,
        elapsed,
        tenants_per_second,
        statuses,
    }
}

/// Create one entity under `tenant`, returning the status (0 on transport error).
pub async fn create_entity(client: &Client, broker_base: &str, tenant: &str, entity: &Value) -> u16 {
    client
        .post(format!("{broker_base}entities"))
        .header("NGSILD-Tenant", tenant)
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .map_or(0, |response| response.status().as_u16())
}

/// The outcome of a bounded load run.
#[derive(Debug, Clone)]
pub struct LoadReport {
    /// Total requests issued.
    pub total: usize,
    /// Requests whose status was outside the 2xx range (transport errors, as
    /// status 0, are included).
    pub non_2xx: usize,
    /// Requests that never got an HTTP response at all.
    pub transport_errors: usize,
    /// Median request latency.
    pub p50: Duration,
    /// 99th-percentile request latency.
    pub p99: Duration,
    /// Slowest request latency.
    pub max: Duration,
    /// Count of responses per status code (transport errors under 0).
    pub statuses: BTreeMap<u16, usize>,
}

impl LoadReport {
    /// Whether every request returned a 2xx status.
    #[must_use]
    pub const fn all_2xx(&self) -> bool {
        self.non_2xx == 0
    }
}

/// Run `workload` on `concurrency` concurrent workers until `duration` elapses,
/// recording every request's latency and status.
///
/// Each worker loops, calling `workload` with a monotonically increasing
/// request index it can use to pick a tenant or shape a request, until the
/// deadline; the returned [`LoadReport`] aggregates latency percentiles and a
/// per-status tally over every request all workers made.
pub async fn run_bounded<F, Fut>(concurrency: usize, duration: Duration, workload: F) -> LoadReport
where
    F: Fn(u64) -> Fut + Sync,
    Fut: Future<Output = u16> + Send,
{
    let deadline = Instant::now() + duration;
    let counter = AtomicU64::new(0);
    let workload = &workload;
    let counter = &counter;
    let per_worker = (0..concurrency).map(|_| async move {
        let mut local: Vec<(Duration, u16)> = Vec::new();
        while Instant::now() < deadline {
            let index = counter.fetch_add(1, Ordering::Relaxed);
            let started = Instant::now();
            let status = workload(index).await;
            local.push((started.elapsed(), status));
        }
        local
    });
    let mut samples: Vec<(Duration, u16)> = join_all(per_worker).await.into_iter().flatten().collect();

    let total = samples.len();
    let non_2xx = samples.iter().filter(|(_, status)| !(200..300).contains(status)).count();
    let transport_errors = samples.iter().filter(|(_, status)| *status == 0).count();
    let mut statuses: BTreeMap<u16, usize> = BTreeMap::new();
    for (_, status) in &samples {
        *statuses.entry(*status).or_insert(0) += 1;
    }
    samples.sort_by_key(|(latency, _)| *latency);
    let latencies: Vec<Duration> = samples.into_iter().map(|(latency, _)| latency).collect();
    LoadReport {
        total,
        non_2xx,
        transport_errors,
        p50: nth_percentile(&latencies, 50),
        p99: nth_percentile(&latencies, 99),
        max: latencies.last().copied().unwrap_or(Duration::ZERO),
        statuses,
    }
}

/// A small, fast, deterministic PRNG (`SplitMix64`), enough to spread a picker's
/// choices without pulling in a random-number dependency.
struct SplitMix64 {
    /// The evolving generator state.
    state: u64,
}

impl SplitMix64 {
    /// A generator seeded with `seed`.
    const fn new(seed: u64) -> SplitMix64 {
        SplitMix64 { state: seed }
    }

    /// The next 64-bit output, advancing the state.
    const fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform float in `[0, 1)` with 24 bits of resolution.
    fn uniform01(&mut self) -> f64 {
        let bits = self.next_u64() >> 40;
        let numerator = u32::try_from(bits).unwrap_or(0);
        f64::from(numerator) / f64::from(1u32 << 24)
    }
}

/// Picks tenant indices uniformly at random over `0..count`.
pub struct UniformPicker {
    /// The generator behind the picker.
    rng: Mutex<SplitMix64>,
    /// How many tenants the picker chooses among.
    count: usize,
}

impl UniformPicker {
    /// A uniform picker over `count` tenants, seeded with `seed`.
    #[must_use]
    pub const fn new(count: usize, seed: u64) -> UniformPicker {
        UniformPicker {
            rng: Mutex::new(SplitMix64::new(seed)),
            count,
        }
    }

    /// The next uniformly chosen tenant index.
    #[must_use]
    pub fn pick(&self) -> usize {
        let Ok(mut rng) = self.rng.lock() else {
            return 0;
        };
        let modulus = u64::try_from(self.count.max(1)).unwrap_or(1);
        usize::try_from(rng.next_u64() % modulus).unwrap_or(0)
    }
}

/// Picks tenant indices from a Zipf distribution over `0..count`, so a few
/// low-index tenants are hot and the rest are cold.
pub struct ZipfPicker {
    /// The generator behind the picker.
    rng: Mutex<SplitMix64>,
    /// Normalised cumulative weights; `pick` searches these.
    cumulative: Vec<f64>,
}

impl ZipfPicker {
    /// A Zipf picker over `count` tenants with exponent `exponent`, seeded with
    /// `seed`. Index 0 is the hottest.
    #[must_use]
    pub fn new(count: usize, exponent: f64, seed: u64) -> ZipfPicker {
        let weights: Vec<f64> = (0..count.max(1))
            .map(|index| {
                let rank = as_f64(index + 1);
                1.0 / rank.powf(exponent)
            })
            .collect();
        let total: f64 = weights.iter().sum();
        let mut running = 0.0;
        let cumulative = weights
            .iter()
            .map(|weight| {
                running += weight / total;
                running
            })
            .collect();
        ZipfPicker {
            rng: Mutex::new(SplitMix64::new(seed)),
            cumulative,
        }
    }

    /// The next Zipf-distributed tenant index.
    #[must_use]
    pub fn pick(&self) -> usize {
        let Ok(mut rng) = self.rng.lock() else {
            return 0;
        };
        let sample = rng.uniform01();
        let index = self.cumulative.partition_point(|cumulative| *cumulative < sample);
        index.min(self.cumulative.len().saturating_sub(1))
    }
}

/// The `pct`-th percentile of a latency slice already sorted ascending, by
/// integer index arithmetic (no float casts).
fn nth_percentile(sorted: &[Duration], pct: usize) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let index = sorted.len().saturating_sub(1).saturating_mul(pct) / 100;
    sorted.get(index).copied().unwrap_or(Duration::ZERO)
}

/// A lossless-enough `usize` to `f64` conversion that stays clear of the cast
/// lints; counts in this tier never approach `u32::MAX`.
fn as_f64(value: usize) -> f64 {
    u32::try_from(value).map_or(f64::from(u32::MAX), f64::from)
}
