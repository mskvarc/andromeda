//! Gauges describing the on-disk size of the live schema's base tables.

use crate::{
    names::{DB_TABLE_BYTES, DB_TABLE_INDEX_BYTES, LABEL_TABLE, LABEL_TENANT},
    sample_value::SampleValue,
};
use metrics::gauge;

/// A point-in-time size sample of one base table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableSizeSample {
    /// Total on-disk bytes, indexes and TOAST included.
    pub total_bytes: u64,
    /// On-disk bytes held by the table's indexes.
    pub index_bytes: u64,
}

/// The size gauges one tenant's tables publish.
///
/// The tenant label is fixed at construction; the table label is supplied per
/// sample and is drawn from a fixed, bounded set of known base tables, so the
/// series count stays low even across many tenants.
#[derive(Debug, Clone)]
pub struct StorageGauges {
    /// The `tenant` label value.
    tenant: String,
}

impl StorageGauges {
    /// Gauges for the tables served by `tenant`.
    ///
    /// The label is free text because it is a rendered identifier, not a domain
    /// value: the caller renders its tenant type into it once.
    pub fn for_tenant(tenant: impl Into<String>) -> StorageGauges {
        StorageGauges { tenant: tenant.into() }
    }

    /// Publish one table's `sample` on both gauges under `table`.
    ///
    /// The `table` label is a fixed base-table name the caller supplies from a
    /// closed set, not free-form input.
    pub fn record(&self, table: &'static str, sample: TableSizeSample) {
        // clone: the metrics facade owns each label value it stores.
        gauge!(DB_TABLE_BYTES, LABEL_TENANT => self.tenant.clone(), LABEL_TABLE => table).set(SampleValue::from(sample.total_bytes));
        gauge!(DB_TABLE_INDEX_BYTES, LABEL_TENANT => self.tenant.clone(), LABEL_TABLE => table).set(SampleValue::from(sample.index_bytes));
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TelemetryError,
        names::{DB_TABLE_BYTES, DB_TABLE_INDEX_BYTES},
        recorder::TelemetryRecorder,
        storage::{StorageGauges, TableSizeSample},
    };

    #[test]
    fn a_sample_sets_both_gauges_under_the_tenant_and_table_labels() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            StorageGauges::for_tenant("default").record(
                "current_attributes",
                TableSizeSample {
                    total_bytes: 4096,
                    index_bytes: 1024,
                },
            );
        });
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{DB_TABLE_BYTES}{{tenant=\"default\",table=\"current_attributes\"}} 4096")),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!("{DB_TABLE_INDEX_BYTES}{{tenant=\"default\",table=\"current_attributes\"}} 1024")),
            "{rendered}"
        );
        Ok(())
    }
}
