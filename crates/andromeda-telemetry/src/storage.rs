//! Series describing the on-disk size of the live schema's base tables, the
//! updates they took and how many of those were heap-only, and the size of the
//! history chunk each temporal hypertable converts next.
//!
//! PostgreSQL keeps each table's update counts as cumulative statistics that a
//! reset or a crash recovery sets back to zero. They are exposed as counters
//! the broker advances by the difference between two samples, so a reset never
//! moves a counter backwards and `rate()` reads the updates of any window.

use crate::{
    names::{
        LABEL_TABLE,
        LABEL_TENANT,
        POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES,
        POSTGRESQL_TABLE_BYTES,
        POSTGRESQL_TABLE_HOT_UPDATES_TOTAL,
        POSTGRESQL_TABLE_INDEX_BYTES,
        POSTGRESQL_TABLE_UPDATES_TOTAL,
    },
    sample_value::SampleValue,
    tenant_label::TenantLabel,
};
use dashmap::DashMap;
use metrics::{counter, gauge};
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

/// The last sample published for each of one tenant's tables.
#[derive(Debug, Default)]
struct PublishedSamples {
    /// The last size sample of each base table, by its `table` label.
    tables: DashMap<&'static str, TableSizeSample>,
    /// The last largest-rowstore-chunk size of each history hypertable, in
    /// bytes, by its `table` label.
    rowstore_chunks: DashMap<&'static str, u64>,
}

/// The size and update series one tenant's tables publish.
///
/// The tenant label is fixed at construction; the table label is supplied per
/// sample and is drawn from a fixed, bounded set of known base tables, so the
/// series count stays low even across many tenants. Every clone shares the last
/// samples published, so [`StorageMetrics::republish`] keeps them in the
/// exposition while no new sample is taken. Once nothing republishes them (the
/// tenant was unloaded), the idle timeout reaps them.
#[derive(Debug, Clone)]
pub struct StorageMetrics {
    /// The `tenant` label value.
    tenant: TenantLabel,
    /// The last samples published, shared by every clone.
    published: Arc<PublishedSamples>,
}

impl StorageMetrics {
    /// Series for the tables served by `tenant`, with nothing published yet.
    #[must_use]
    pub fn for_tenant(tenant: TenantLabel) -> StorageMetrics {
        StorageMetrics {
            tenant,
            published: Arc::default(),
        }
    }

    /// Publish one table's `sample` on every series under `table`.
    ///
    /// The first sample sets the update counters to the table's statistics, so
    /// a tenant loaded again while its series are still exposed does not count
    /// its updates twice. Every later sample advances them by what the
    /// statistics moved since the one before. The `table` label is a fixed
    /// base-table name the caller supplies from a closed set, not free-form
    /// input.
    pub fn record(&self, table: &'static str, sample: TableSizeSample) {
        self.set_sizes(table, sample);
        let updates = counter!(POSTGRESQL_TABLE_UPDATES_TOTAL, LABEL_TENANT => self.tenant.value(), LABEL_TABLE => table);
        let hot_updates = counter!(POSTGRESQL_TABLE_HOT_UPDATES_TOTAL, LABEL_TENANT => self.tenant.value(), LABEL_TABLE => table);
        match self.published.tables.insert(table, sample) {
            None => {
                updates.absolute(sample.updated_tuples);
                hot_updates.absolute(sample.hot_updated_tuples);
            }
            Some(previous) => {
                updates.increment(advance(previous.updated_tuples, sample.updated_tuples));
                hot_updates.increment(advance(previous.hot_updated_tuples, sample.hot_updated_tuples));
            }
        }
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
            self.set_sizes(sample.key(), *sample.value());
            counter!(POSTGRESQL_TABLE_UPDATES_TOTAL, LABEL_TENANT => self.tenant.value(), LABEL_TABLE => *sample.key()).increment(0);
            counter!(POSTGRESQL_TABLE_HOT_UPDATES_TOTAL, LABEL_TENANT => self.tenant.value(), LABEL_TABLE => *sample.key()).increment(0);
        }
        for chunk in &self.published.rowstore_chunks {
            self.set_rowstore_chunk(chunk.key(), *chunk.value());
        }
    }

    /// Set both size gauges of base table `table` to `sample`.
    fn set_sizes(&self, table: &'static str, sample: TableSizeSample) {
        gauge!(POSTGRESQL_TABLE_BYTES, LABEL_TENANT => self.tenant.value(), LABEL_TABLE => table).set(SampleValue::from(sample.total_bytes));
        gauge!(POSTGRESQL_TABLE_INDEX_BYTES, LABEL_TENANT => self.tenant.value(), LABEL_TABLE => table).set(SampleValue::from(sample.index_bytes));
    }

    /// Set the largest-rowstore-chunk gauge of history hypertable `table` to
    /// `bytes`.
    fn set_rowstore_chunk(&self, table: &'static str, bytes: u64) {
        gauge!(POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES, LABEL_TENANT => self.tenant.value(), LABEL_TABLE => table).set(SampleValue::from(bytes));
    }
}

/// How far a cumulative statistics count moved from `previous` to `current`:
/// the difference, or all of `current` when the statistics were reset between
/// the two samples.
const fn advance(previous: u64, current: u64) -> u64 {
    match current.checked_sub(previous) {
        Some(difference) => difference,
        None => current,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TelemetryError,
        names::{
            POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES,
            POSTGRESQL_TABLE_BYTES,
            POSTGRESQL_TABLE_HOT_UPDATES_TOTAL,
            POSTGRESQL_TABLE_INDEX_BYTES,
            POSTGRESQL_TABLE_UPDATES_TOTAL,
        },
        recorder::TelemetryRecorder,
        storage::{StorageMetrics, TableSizeSample},
        tenant_label::TenantLabel,
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

    /// A sample of 4096 bytes whose statistics count `updated` updates, `hot` of them heap-only.
    const fn updates_of(updated: u64, hot: u64) -> TableSizeSample {
        TableSizeSample {
            total_bytes: 4096,
            index_bytes: 1024,
            updated_tuples: updated,
            hot_updated_tuples: hot,
        }
    }

    /// Metrics for the unnamed default tenant.
    fn default_tenant() -> StorageMetrics {
        StorageMetrics::for_tenant(TenantLabel::unnamed_default())
    }

    /// Whether `rendered` holds `series` as a whole line.
    fn holds(rendered: &str, series: &str) -> bool {
        rendered.lines().any(|line| line == series)
    }

    /// The update counters of the default tenant's `entities` table, as rendered.
    fn update_counters(updated: u64, hot: u64) -> [String; 2] {
        [
            format!("{POSTGRESQL_TABLE_UPDATES_TOTAL}{{tenant=\"@default\",table=\"entities\"}} {updated}"),
            format!("{POSTGRESQL_TABLE_HOT_UPDATES_TOTAL}{{tenant=\"@default\",table=\"entities\"}} {hot}"),
        ]
    }

    /// The sampling task republishes on every tick that takes no sample, so
    /// the last sample outlives the idle timeout while the task runs.
    #[test]
    fn a_republished_sample_survives_the_idle_timeout_and_a_clone_shares_it() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached_with(Duration::from_millis(50), None)?;
        let metrics = default_tenant();
        recorder.record_locally(|| {
            metrics.record("entities", updates_of(8, 6));
            metrics.record_largest_rowstore_chunk("attribute_history", 600_000_000);
        });
        for _ in 0..3 {
            sleep(Duration::from_millis(50));
            recorder.record_locally(|| metrics.clone().republish());
        }
        let rendered = recorder.render();
        let [updates, hot_updates] = update_counters(8, 6);
        for series in [
            format!("{POSTGRESQL_TABLE_BYTES}{{tenant=\"@default\",table=\"entities\"}} 4096"),
            format!("{POSTGRESQL_TABLE_INDEX_BYTES}{{tenant=\"@default\",table=\"entities\"}} 1024"),
            updates,
            hot_updates,
            format!("{POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES}{{tenant=\"@default\",table=\"attribute_history\"}} 600000000"),
        ] {
            assert!(holds(&rendered, &series), "{series}: {rendered}");
        }
        Ok(())
    }

    #[test]
    fn a_new_sample_replaces_the_one_republished() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        let metrics = default_tenant();
        recorder.record_locally(|| {
            metrics.record("entities", sample_of(8192));
            metrics.record("entities", sample_of(16384));
            metrics.republish();
        });
        let rendered = recorder.render();
        let series = format!("{POSTGRESQL_TABLE_BYTES}{{tenant=\"@default\",table=\"entities\"}} 16384");
        assert!(holds(&rendered, &series), "{rendered}");
        Ok(())
    }

    #[test]
    fn republishing_before_any_sample_publishes_nothing() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| default_tenant().republish());
        pretty_assertions::assert_eq!(recorder.render().trim(), "");
        Ok(())
    }

    #[test]
    fn the_largest_rowstore_chunk_is_published_under_the_tenant_and_hypertable_labels() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            let metrics = StorageMetrics::for_tenant(TenantLabel::named("acme"));
            metrics.record_largest_rowstore_chunk("attribute_history", 600_000_000);
            metrics.record_largest_rowstore_chunk("scope_history", 0);
        });
        let rendered = recorder.render();
        for series in [
            format!("{POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES}{{tenant=\"acme\",table=\"attribute_history\"}} 600000000"),
            format!("{POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES}{{tenant=\"acme\",table=\"scope_history\"}} 0"),
        ] {
            assert!(holds(&rendered, &series), "{series}: {rendered}");
        }
        Ok(())
    }

    #[test]
    fn a_first_sample_sets_every_series_under_the_tenant_and_table_labels() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| default_tenant().record("entities", updates_of(8, 6)));
        let rendered = recorder.render();
        let [updates, hot_updates] = update_counters(8, 6);
        for series in [
            format!("{POSTGRESQL_TABLE_BYTES}{{tenant=\"@default\",table=\"entities\"}} 4096"),
            format!("{POSTGRESQL_TABLE_INDEX_BYTES}{{tenant=\"@default\",table=\"entities\"}} 1024"),
            updates,
            hot_updates,
        ] {
            assert!(holds(&rendered, &series), "{series}: {rendered}");
        }
        assert!(rendered.contains(&format!("# TYPE {POSTGRESQL_TABLE_UPDATES_TOTAL} counter")), "{rendered}");
        Ok(())
    }

    #[test]
    fn the_update_counters_advance_by_what_the_statistics_moved_between_samples() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            let metrics = default_tenant();
            metrics.record("entities", updates_of(8, 6));
            metrics.record("entities", updates_of(20, 10));
        });
        let rendered = recorder.render();
        for series in update_counters(20, 10) {
            assert!(holds(&rendered, &series), "{series}: {rendered}");
        }
        Ok(())
    }

    /// 100 then 5 updates: the statistics were reset in between, so the 5
    /// counted since the reset are added to the 100 already counted.
    #[test]
    fn a_statistics_reset_advances_the_update_counters_by_what_was_counted_since() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            let metrics = default_tenant();
            metrics.record("entities", updates_of(100, 50));
            metrics.record("entities", updates_of(5, 1));
        });
        let rendered = recorder.render();
        for series in update_counters(105, 51) {
            assert!(holds(&rendered, &series), "{series}: {rendered}");
        }
        Ok(())
    }

    /// The tenant is loaded again before its series were reaped: the new
    /// sampler's first sample sets the counters to 120 and 60 rather than
    /// adding them to the 100 and 50 already exposed.
    #[test]
    fn a_tenant_loaded_again_does_not_count_its_updates_twice() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            default_tenant().record("entities", updates_of(100, 50));
            default_tenant().record("entities", updates_of(120, 60));
        });
        let rendered = recorder.render();
        for series in update_counters(120, 60) {
            assert!(holds(&rendered, &series), "{series}: {rendered}");
        }
        Ok(())
    }
}
