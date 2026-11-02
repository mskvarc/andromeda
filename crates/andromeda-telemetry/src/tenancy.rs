//! Metrics describing the tenant-runtime cache.
//!
//! How many runtimes are held, how cold builds fare and how long they wait for
//! admission, which waits shed requests as saturated, why runtimes are
//! evicted, and how dormant-tenant wake attempts resolve. None of these series carries a tenant label: a broker holding
//! thousands of tenants would otherwise grow one series per tenant per metric.
//! The per-tenant pool and storage series that do carry a tenant label
//! ([`crate::pool`], [`crate::storage`]) are bounded instead by the recorder's
//! idle timeout, which reaps a tenant's series once eviction stops it being
//! sampled (see [`crate::recorder`]).

use crate::{
    names::{
        LABEL_OUTCOME,
        LABEL_REASON,
        LABEL_WAIT,
        TENANT_BUILD_ADMISSION_WAIT_DURATION_SECONDS,
        TENANT_COLD_BUILD_DURATION_SECONDS,
        TENANT_COLD_BUILDS_TOTAL,
        TENANT_EVICTIONS_TOTAL,
        TENANT_REQUESTS_SHED_TOTAL,
        TENANT_RUNTIMES_CACHED,
        TENANT_WAKES_TOTAL,
    },
    operation::Outcome,
    sample_value::SampleValue,
};
use metrics::{counter, gauge, histogram};
use strum::IntoStaticStr;

/// Why a tenant runtime left the cache.
///
/// Rendered as the kebab-case `reason` label value, so no free-form string ever
/// becomes a series dimension.
#[derive(Clone, Copy, Debug, Eq, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum EvictionReason {
    /// The runtime sat idle past the runtime idle TTL.
    Idle,
    /// The cache was over its cap and this was the least-recently-used entry.
    Excess,
    /// The tenant was evicted so its due work runs from the wake schedule.
    Dormant,
    /// The tenant's registry row was removed, so its runtime is orphaned.
    Orphaned,
    /// The tenant was administratively suspended, so its runtime is released at
    /// once and never rescheduled for a wake while it stays suspended.
    Suspended,
}

/// How a dormant-tenant wake attempt resolved.
///
/// Rendered as the kebab-case `outcome` label value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum WakeOutcome {
    /// The tenant was already cached, so no load was needed.
    AlreadyCached,
    /// The runtime was cold-loaded to run its due work.
    Loaded,
    /// Nothing was due, so no runtime was loaded.
    NothingDue,
    /// The cache was full of fresher runtimes, so the wake was deferred.
    Deferred,
    /// The named tenant is not registered.
    Unregistered,
}

/// Set the gauge of tenant runtimes currently held in the cache.
pub fn set_runtimes_cached(count: usize) {
    gauge!(TENANT_RUNTIMES_CACHED).set(SampleValue::from(count));
}

/// Record a cold tenant-runtime build's outcome and its wall-clock seconds:
/// [`Outcome::Ok`] when the runtime was built and cached.
pub fn record_cold_build(outcome: Outcome, seconds: f64) {
    let outcome: &'static str = outcome.into();
    counter!(TENANT_COLD_BUILDS_TOTAL, LABEL_OUTCOME => outcome).increment(1);
    histogram!(TENANT_COLD_BUILD_DURATION_SECONDS).record(seconds);
}

/// Record the seconds a cold build waited for a build-admission permit.
pub fn record_build_admission_wait(seconds: f64) {
    histogram!(TENANT_BUILD_ADMISSION_WAIT_DURATION_SECONDS).record(seconds);
}

/// Count one request shed as saturated, under the wait that shed it.
///
/// The label is rendered by the caller from its own wait type, so this crate
/// stays a leaf that names no tenancy type.
pub fn record_shed_request(wait: &'static str) {
    counter!(TENANT_REQUESTS_SHED_TOTAL, LABEL_WAIT => wait).increment(1);
}

/// Count one tenant-runtime eviction with its reason.
pub fn record_eviction(reason: EvictionReason) {
    let reason: &'static str = reason.into();
    counter!(TENANT_EVICTIONS_TOTAL, LABEL_REASON => reason).increment(1);
}

/// Count one dormant-tenant wake attempt with its outcome.
pub fn record_wake(outcome: WakeOutcome) {
    let outcome: &'static str = outcome.into();
    counter!(TENANT_WAKES_TOTAL, LABEL_OUTCOME => outcome).increment(1);
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TelemetryError,
        names::{TENANT_COLD_BUILDS_TOTAL, TENANT_EVICTIONS_TOTAL, TENANT_REQUESTS_SHED_TOTAL, TENANT_RUNTIMES_CACHED, TENANT_WAKES_TOTAL},
        operation::Outcome,
        recorder::TelemetryRecorder,
        tenancy::{EvictionReason, WakeOutcome, record_cold_build, record_eviction, record_shed_request, record_wake, set_runtimes_cached},
    };

    #[test]
    fn eviction_reasons_render_as_kebab_case_label_values() {
        for (reason, expected) in [
            (EvictionReason::Idle, "idle"),
            (EvictionReason::Excess, "excess"),
            (EvictionReason::Dormant, "dormant"),
            (EvictionReason::Orphaned, "orphaned"),
            (EvictionReason::Suspended, "suspended"),
        ] {
            let rendered: &'static str = reason.into();
            assert_eq!(rendered, expected);
        }
    }

    #[test]
    fn wake_outcomes_render_as_kebab_case_label_values() {
        for (outcome, expected) in [
            (WakeOutcome::AlreadyCached, "already-cached"),
            (WakeOutcome::Loaded, "loaded"),
            (WakeOutcome::NothingDue, "nothing-due"),
            (WakeOutcome::Deferred, "deferred"),
            (WakeOutcome::Unregistered, "unregistered"),
        ] {
            let rendered: &'static str = outcome.into();
            assert_eq!(rendered, expected);
        }
    }

    #[test]
    fn the_recording_functions_emit_the_named_series_with_their_labels() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            set_runtimes_cached(12);
            record_cold_build(Outcome::Ok, 0.5);
            record_eviction(EvictionReason::Excess);
            record_wake(WakeOutcome::Loaded);
            record_shed_request("migration-pool");
            record_shed_request("migration-pool");
        });
        let rendered = recorder.render();
        assert!(rendered.contains(&format!("{TENANT_RUNTIMES_CACHED} 12")), "{rendered}");
        assert!(rendered.contains(&format!("{TENANT_COLD_BUILDS_TOTAL}{{outcome=\"ok\"}} 1")), "{rendered}");
        assert!(rendered.contains(&format!("{TENANT_EVICTIONS_TOTAL}{{reason=\"excess\"}} 1")), "{rendered}");
        assert!(rendered.contains(&format!("{TENANT_WAKES_TOTAL}{{outcome=\"loaded\"}} 1")), "{rendered}");
        assert!(
            rendered.contains(&format!("{TENANT_REQUESTS_SHED_TOTAL}{{wait=\"migration-pool\"}} 2")),
            "{rendered}"
        );
        Ok(())
    }
}
