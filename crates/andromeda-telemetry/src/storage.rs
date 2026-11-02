//! Gauges describing the on-disk size of the live schema's base tables, the
//! share of their updates that were heap-only, and the size of the history
//! chunk each temporal hypertable converts next.

use crate::{
    names::{
        LABEL_TABLE,
        LABEL_TENANT,
        POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES,
        POSTGRESQL_TABLE_BYTES,
        POSTGRESQL_TABLE_HOT_UPDATE_RATIO,
        POSTGRESQL_TABLE_INDEX_BYTES,
    },
    sample_value::SampleValue,
};
use metrics::{IntoF64, gauge};

/// A point-in-time size and update sample of one base table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableSizeSample {
    /// Total on-disk bytes, indexes and TOAST included.
    pub total_bytes: u64,
    /// On-disk bytes held by the table's indexes.
    pub index_bytes: u64,
    /// Rows updated since the table's statistics were reset.
    pub updated_tuples: u64,
    /// The updates among them that were heap-only.
    pub hot_updated_tuples: u64,
}

impl TableSizeSample {
    /// Heap-only updates as a share of all updates, 0 for a table that took
    /// none.
    fn hot_update_ratio(self) -> f64 {
        match self.updated_tuples {
            0 => 0.0,
            updated => SampleValue::from(self.hot_updated_tuples).into_f64() / SampleValue::from(updated).into_f64(),
        }
    }
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

    /// Publish one table's `sample` on every gauge under `table`.
    ///
    /// The `table` label is a fixed base-table name the caller supplies from a
    /// closed set, not free-form input.
    pub fn record(&self, table: &'static str, sample: TableSizeSample) {
        // clone: the metrics facade owns each label value it stores.
        gauge!(POSTGRESQL_TABLE_BYTES, LABEL_TENANT => self.tenant.clone(), LABEL_TABLE => table).set(SampleValue::from(sample.total_bytes));
        gauge!(POSTGRESQL_TABLE_INDEX_BYTES, LABEL_TENANT => self.tenant.clone(), LABEL_TABLE => table).set(SampleValue::from(sample.index_bytes));
        gauge!(POSTGRESQL_TABLE_HOT_UPDATE_RATIO, LABEL_TENANT => self.tenant.clone(), LABEL_TABLE => table).set(sample.hot_update_ratio());
    }

    /// Publish the `bytes` of the largest chunk of history hypertable `table`
    /// still in the rowstore.
    ///
    /// The `table` label is a fixed hypertable name the caller supplies from a
    /// closed set, not free-form input.
    pub fn record_largest_rowstore_chunk(&self, table: &'static str, bytes: u64) {
        // clone: the metrics facade owns each label value it stores.
        gauge!(POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES, LABEL_TENANT => self.tenant.clone(), LABEL_TABLE => table).set(SampleValue::from(bytes));
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TelemetryError,
        names::{POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES, POSTGRESQL_TABLE_BYTES, POSTGRESQL_TABLE_HOT_UPDATE_RATIO, POSTGRESQL_TABLE_INDEX_BYTES},
        recorder::TelemetryRecorder,
        storage::{StorageGauges, TableSizeSample},
    };

    #[test]
    fn the_largest_rowstore_chunk_is_published_under_the_tenant_and_hypertable_labels() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            let gauges = StorageGauges::for_tenant("default");
            gauges.record_largest_rowstore_chunk("attribute_history", 600_000_000);
            gauges.record_largest_rowstore_chunk("scope_history", 0);
        });
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!(
                "{POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES}{{tenant=\"default\",table=\"attribute_history\"}} 600000000"
            )),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!(
                "{POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES}{{tenant=\"default\",table=\"scope_history\"}} 0"
            )),
            "{rendered}"
        );
        Ok(())
    }

    #[test]
    fn a_sample_sets_every_gauge_under_the_tenant_and_table_labels() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            StorageGauges::for_tenant("default").record(
                "current_attributes",
                TableSizeSample {
                    total_bytes: 4096,
                    index_bytes: 1024,
                    updated_tuples: 8,
                    hot_updated_tuples: 6,
                },
            );
        });
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{POSTGRESQL_TABLE_BYTES}{{tenant=\"default\",table=\"current_attributes\"}} 4096")),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!(
                "{POSTGRESQL_TABLE_INDEX_BYTES}{{tenant=\"default\",table=\"current_attributes\"}} 1024"
            )),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!(
                "{POSTGRESQL_TABLE_HOT_UPDATE_RATIO}{{tenant=\"default\",table=\"current_attributes\"}} 0.75"
            )),
            "{rendered}"
        );
        Ok(())
    }

    #[test]
    fn hot_ratio_is_zero_without_updates() {
        let sample = TableSizeSample {
            total_bytes: 4096,
            index_bytes: 1024,
            updated_tuples: 0,
            hot_updated_tuples: 0,
        };
        pretty_assertions::assert_eq!(sample.hot_update_ratio().to_bits(), 0.0_f64.to_bits());
    }

    #[test]
    fn hot_ratio_is_the_heap_only_share_of_updates() {
        let sample = TableSizeSample {
            total_bytes: 4096,
            index_bytes: 1024,
            updated_tuples: 4,
            hot_updated_tuples: 1,
        };
        pretty_assertions::assert_eq!(sample.hot_update_ratio().to_bits(), 0.25_f64.to_bits());
    }
}
