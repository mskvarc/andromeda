//! Gauges describing one connection pool.

use crate::{
    names::{LABEL_TENANT, POSTGRESQL_POOL_CONNECTIONS, POSTGRESQL_POOL_IDLE_CONNECTIONS},
    sample_value::SampleValue,
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
    tenant: String,
}

impl PoolGauges {
    /// Gauges for the pool serving `tenant`.
    ///
    /// The label is free text because it is a rendered identifier, not a
    /// domain value: the caller renders its tenant type into it once.
    pub fn for_tenant(tenant: impl Into<String>) -> PoolGauges {
        PoolGauges { tenant: tenant.into() }
    }

    /// Publish `sample` on both gauges.
    pub fn record(&self, sample: PoolSample) {
        // clone: the metrics facade owns each label value it stores.
        gauge!(POSTGRESQL_POOL_CONNECTIONS, LABEL_TENANT => self.tenant.clone()).set(f64::from(sample.open));
        gauge!(POSTGRESQL_POOL_IDLE_CONNECTIONS, LABEL_TENANT => self.tenant.clone()).set(SampleValue::from(sample.idle));
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TelemetryError,
        names::{POSTGRESQL_POOL_CONNECTIONS, POSTGRESQL_POOL_IDLE_CONNECTIONS},
        pool::{PoolGauges, PoolSample},
        recorder::TelemetryRecorder,
    };

    #[test]
    fn a_sample_sets_both_gauges_under_the_tenant_label() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| PoolGauges::for_tenant("default").record(PoolSample { open: 8, idle: 3 }));
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{POSTGRESQL_POOL_CONNECTIONS}{{tenant=\"default\"}} 8")),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!("{POSTGRESQL_POOL_IDLE_CONNECTIONS}{{tenant=\"default\"}} 3")),
            "{rendered}"
        );
        Ok(())
    }
}
