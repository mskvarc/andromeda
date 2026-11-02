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
use dashmap::DashMap;
use metrics::{IntoF64, gauge};
use std::sync::Arc;

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

/// The last sample published for each of one tenant's tables.
#[derive(Debug, Default)]
struct PublishedSamples {
    /// The last size sample of each base table, by its `table` label.
    tables: DashMap<&'static str, TableSizeSample>,
    /// The last largest-rowstore-chunk size of each history hypertable, in
    /// bytes, by its `table` label.
    rowstore_chunks: DashMap<&'static str, u64>,
}

/// The size gauges one tenant's tables publish.
///
/// The tenant label is fixed at construction; the table label is supplied per
/// sample and is drawn from a fixed, bounded set of known base tables, so the
/// series count stays low even across many tenants. Every clone shares the last
/// samples published, so [`StorageGauges::republish`] keeps them in the
/// exposition while no new sample is taken. Once nothing republishes them (the
/// tenant was unloaded), the gauge idle timeout reaps them.
#[derive(Debug, Clone)]
pub struct StorageGauges {
    /// The `tenant` label value.
    tenant: String,
    /// The last samples published, shared by every clone.
    published: Arc<PublishedSamples>,
}

impl StorageGauges {
    /// Gauges for the tables served by `tenant`, with nothing published yet.
    ///
    /// The label is free text because it is a rendered identifier, not a domain
    /// value: the caller renders its tenant type into it once.
    pub fn for_tenant(tenant: impl Into<String>) -> StorageGauges {
        StorageGauges {
            tenant: tenant.into(),
            published: Arc::default(),
        }
    }

    /// Publish one table's `sample` on every gauge under `table`.
    ///
    /// The `table` label is a fixed base-table name the caller supplies from a
    /// closed set, not free-form input.
    pub fn record(&self, table: &'static str, sample: TableSizeSample) {
        self.set_table(table, sample);
        self.published.tables.insert(table, sample);
    }

    /// Publish the `bytes` of the largest chunk of history hypertable `table`
    /// still in the rowstore.
    ///
    /// The `table` label is a fixed hypertable name the caller supplies from a
    /// closed set, not free-form input.
    pub fn record_largest_rowstore_chunk(&self, table: &'static str, bytes: u64) {
        self.set_rowstore_chunk(table, bytes);
        self.published.rowstore_chunks.insert(table, bytes);
    }

    /// Publish every last sample again, unchanged, for a pass that takes no new
    /// sample because nothing was written since the last.
    pub fn republish(&self) {
        for sample in &self.published.tables {
            self.set_table(sample.key(), *sample.value());
        }
        for chunk in &self.published.rowstore_chunks {
            self.set_rowstore_chunk(chunk.key(), *chunk.value());
        }
    }

    /// Set every gauge of base table `table` to `sample`.
    fn set_table(&self, table: &'static str, sample: TableSizeSample) {
        // clone: the metrics facade owns each label value it stores.
        gauge!(POSTGRESQL_TABLE_BYTES, LABEL_TENANT => self.tenant.clone(), LABEL_TABLE => table).set(SampleValue::from(sample.total_bytes));
        gauge!(POSTGRESQL_TABLE_INDEX_BYTES, LABEL_TENANT => self.tenant.clone(), LABEL_TABLE => table).set(SampleValue::from(sample.index_bytes));
        gauge!(POSTGRESQL_TABLE_HOT_UPDATE_RATIO, LABEL_TENANT => self.tenant.clone(), LABEL_TABLE => table).set(sample.hot_update_ratio());
    }

    /// Set the largest-rowstore-chunk gauge of history hypertable `table` to
    /// `bytes`.
    fn set_rowstore_chunk(&self, table: &'static str, bytes: u64) {
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
    use std::{thread::sleep, time::Duration};

    /// A sample of `total_bytes` with a quarter of it in indexes and no updates.
    const fn sample_of(total_bytes: u64) -> TableSizeSample {
        TableSizeSample {
            total_bytes,
            index_bytes: total_bytes / 4,
            updated_tuples: 0,
            hot_updated_tuples: 0,
        }
    }

    /// The sampling task republishes on every tick that takes no sample, so
    /// the last sample outlives the gauge idle timeout while the task runs.
    #[test]
    fn a_republished_sample_survives_the_gauge_idle_timeout_and_a_clone_shares_it() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached_with(Duration::from_millis(50), None)?;
        let gauges = StorageGauges::for_tenant("default");
        recorder.record_locally(|| {
            gauges.record("entities", sample_of(8192));
            gauges.record_largest_rowstore_chunk("attribute_history", 600_000_000);
        });
        for _ in 0..3 {
            sleep(Duration::from_millis(50));
            recorder.record_locally(|| gauges.clone().republish());
        }
        let rendered = recorder.render();
        for series in [
            format!("{POSTGRESQL_TABLE_BYTES}{{tenant=\"default\",table=\"entities\"}} 8192"),
            format!("{POSTGRESQL_TABLE_INDEX_BYTES}{{tenant=\"default\",table=\"entities\"}} 2048"),
            format!("{POSTGRESQL_TABLE_HOT_UPDATE_RATIO}{{tenant=\"default\",table=\"entities\"}} 0"),
            format!("{POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES}{{tenant=\"default\",table=\"attribute_history\"}} 600000000"),
        ] {
            assert!(rendered.contains(&format!("{series}\n")), "{series}: {rendered}");
        }
        Ok(())
    }

    #[test]
    fn a_new_sample_replaces_the_one_republished() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        let gauges = StorageGauges::for_tenant("default");
        recorder.record_locally(|| {
            gauges.record("entities", sample_of(8192));
            gauges.record("entities", sample_of(16384));
            gauges.republish();
        });
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{POSTGRESQL_TABLE_BYTES}{{tenant=\"default\",table=\"entities\"}} 16384\n")),
            "{rendered}"
        );
        Ok(())
    }

    #[test]
    fn republishing_before_any_sample_publishes_nothing() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| StorageGauges::for_tenant("default").republish());
        pretty_assertions::assert_eq!(recorder.render().trim(), "");
        Ok(())
    }

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
