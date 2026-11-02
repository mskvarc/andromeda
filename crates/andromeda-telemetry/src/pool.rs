//! Gauges describing one connection pool.

use crate::{
    names::{LABEL_TENANT, POSTGRESQL_POOL_CONNECTIONS, POSTGRESQL_POOL_IDLE_CONNECTIONS},
    sample_value::SampleValue,
    tenant_label::TenantLabel,
};
use metrics::gauge;

/// A point-in-time sample of one pool's occupancy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolSample {
    /// Connections currently open, whether checked out or idle.
    pub open: u32,
    /// Open connections that are idle in the pool.
    pub idle: usize,
}

/// The gauges one tenant's pool publishes.
///
/// The tenant label is fixed at construction so a sampling worker records
/// without touching any name at run time.
#[derive(Debug, Clone)]
pub struct PoolGauges {
    /// The `tenant` label value.
    tenant: TenantLabel,
}

impl PoolGauges {
    /// Gauges for the pool serving `tenant`.
    #[must_use]
    pub const fn for_tenant(tenant: TenantLabel) -> PoolGauges {
        PoolGauges { tenant }
    }

    /// Publish `sample` on both gauges.
    pub fn record(&self, sample: PoolSample) {
        gauge!(POSTGRESQL_POOL_CONNECTIONS, LABEL_TENANT => self.tenant.value()).set(f64::from(sample.open));
        gauge!(POSTGRESQL_POOL_IDLE_CONNECTIONS, LABEL_TENANT => self.tenant.value()).set(SampleValue::from(sample.idle));
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TelemetryError,
        names::{POSTGRESQL_POOL_CONNECTIONS, POSTGRESQL_POOL_IDLE_CONNECTIONS},
        pool::{PoolGauges, PoolSample},
        recorder::TelemetryRecorder,
        tenant_label::TenantLabel,
    };

    #[test]
    fn a_sample_sets_both_gauges_under_the_tenant_label() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| PoolGauges::for_tenant(TenantLabel::unnamed_default()).record(PoolSample { open: 8, idle: 3 }));
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{POSTGRESQL_POOL_CONNECTIONS}{{tenant=\"@default\"}} 8")),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!("{POSTGRESQL_POOL_IDLE_CONNECTIONS}{{tenant=\"@default\"}} 3")),
            "{rendered}"
        );
        Ok(())
    }
}
