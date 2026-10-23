//! Enabling the `TimescaleDB` columnstore on a hypertable.
//!
//! The columnstore is configured by storage options of the hypertable,
//! `ALTER TABLE ... SET (timescaledb.enable_columnstore, ...)`, which sea-query
//! has no form for. Each option value is a literal naming columns the way
//! `TimescaleDB` parses them.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::quoted_table,
    literal::quoted_text,
};
use sea_query::{DynIden, TableName};
use strum::{Display, IntoStaticStr};

/// The summary one sparse index keeps of each columnstore batch, spelled as
/// `timescaledb.sparse_index` and the catalogue spell it.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "lowercase")]
pub enum SparseIndexKind {
    /// The smallest and largest value in the batch, which a range predicate
    /// is checked against.
    Minmax,
    /// A bloom filter of the batch's values, which an equality predicate is
    /// checked against.
    Bloom,
}

/// The direction one columnstore order-by column sorts in.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SortDirection {
    /// Ascending, the default.
    Ascending,
    /// Descending.
    Descending,
}

/// How a hypertable's columnstore lays its batches out.
#[derive(Clone, Debug)]
pub struct ColumnstoreLayout {
    /// The column each segment holds one value of.
    pub segment_by: DynIden,
    /// The columns each segment is sorted by, in order.
    pub order_by: Vec<(DynIden, SortDirection)>,
    /// The sparse indexes each batch keeps, each over its columns in order.
    pub sparse_indexes: Vec<(SparseIndexKind, Vec<DynIden>)>,
}

/// `ALTER TABLE <hypertable> SET (timescaledb.enable_columnstore,
/// timescaledb.segmentby = ..., timescaledb.orderby = ...,
/// timescaledb.sparse_index = ...)`.
///
/// No `timescaledb.compress_chunk_time_interval` is set, so a conversion never
/// merges a chunk into a converted neighbour.
#[derive(Clone, Debug)]
pub struct EnableColumnstore {
    /// The hypertable configured.
    hypertable: TableName,
    /// The layout it is given.
    layout: ColumnstoreLayout,
}

impl EnableColumnstore {
    /// Enable the columnstore of `hypertable` with `layout`.
    #[must_use]
    pub const fn new(hypertable: TableName, layout: ColumnstoreLayout) -> EnableColumnstore {
        EnableColumnstore { hypertable, layout }
    }
}

/// The names of `columns`, comma-separated, as an option value spells them.
fn column_list<'column>(columns: impl IntoIterator<Item = &'column DynIden>) -> String {
    columns.into_iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}

impl RenderedCommand for EnableColumnstore {
    fn rendered(&self) -> String {
        let layout = &self.layout;
        let order_by = layout
            .order_by
            .iter()
            .map(|(column, direction)| match direction {
                SortDirection::Ascending => column.to_string(),
                SortDirection::Descending => format!("{column} DESC"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let sparse_index = layout
            .sparse_indexes
            .iter()
            .map(|(kind, columns)| format!("{kind}({})", column_list(columns)))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "ALTER TABLE {} SET (timescaledb.enable_columnstore, timescaledb.segmentby = {}, timescaledb.orderby = {}, timescaledb.sparse_index = {})",
            quoted_table(&self.hypertable),
            quoted_text(&layout.segment_by.to_string()),
            quoted_text(&order_by),
            quoted_text(&sparse_index),
        )
    }
}

impl Command for EnableColumnstore {}

/// `ALTER TABLE <hypertable> SET (timescaledb.enable_columnstore = false)`:
/// the hypertable stops converting chunks to the columnstore.
#[derive(Clone, Debug)]
pub struct DisableColumnstore {
    /// The hypertable configured.
    hypertable: TableName,
}

impl DisableColumnstore {
    /// Disable the columnstore of `hypertable`.
    #[must_use]
    pub const fn new(hypertable: TableName) -> DisableColumnstore {
        DisableColumnstore { hypertable }
    }
}

impl RenderedCommand for DisableColumnstore {
    fn rendered(&self) -> String {
        format!("ALTER TABLE {} SET (timescaledb.enable_columnstore = false)", quoted_table(&self.hypertable))
    }
}

impl Command for DisableColumnstore {}

/// `ALTER TABLE <hypertable> SET (timescaledb.compress_chunk_time_interval =
/// '0')`: conversions stop merging chunks into a converted neighbour.
///
/// `TimescaleDB` refuses to `RESET` the option and stores a zero interval as
/// none at all. Chunks merged before stay merged.
#[derive(Clone, Debug)]
pub struct ClearColumnstoreMerge {
    /// The hypertable configured.
    hypertable: TableName,
}

impl ClearColumnstoreMerge {
    /// Stop `hypertable`'s conversions from merging chunks.
    #[must_use]
    pub const fn new(hypertable: TableName) -> ClearColumnstoreMerge {
        ClearColumnstoreMerge { hypertable }
    }
}

impl RenderedCommand for ClearColumnstoreMerge {
    fn rendered(&self) -> String {
        format!(
            "ALTER TABLE {} SET (timescaledb.compress_chunk_time_interval = '0')",
            quoted_table(&self.hypertable)
        )
    }
}

impl Command for ClearColumnstoreMerge {}

#[cfg(test)]
mod tests {
    use crate::{
        columnstore_enablement::{ClearColumnstoreMerge, ColumnstoreLayout, DisableColumnstore, EnableColumnstore, SortDirection, SparseIndexKind},
        command::rendering::RenderedCommand,
    };
    use sea_query::{Alias, DynIden, IntoIden, TableName};

    #[test]
    fn disabling_names_the_hypertable_and_turns_the_option_off() {
        pretty_assertions::assert_eq!(
            DisableColumnstore::new(TableName::from(Alias::new("attribute_history"))).rendered(),
            r#"ALTER TABLE "attribute_history" SET (timescaledb.enable_columnstore = false)"#
        );
    }

    #[test]
    fn clearing_the_merge_sets_a_zero_interval_on_the_hypertable() {
        pretty_assertions::assert_eq!(
            ClearColumnstoreMerge::new(TableName::from(Alias::new("attribute_history"))).rendered(),
            r#"ALTER TABLE "attribute_history" SET (timescaledb.compress_chunk_time_interval = '0')"#
        );
    }

    fn column(name: &str) -> DynIden {
        Alias::new(name).into_iden()
    }

    #[test]
    fn the_layout_renders_as_the_hypertable_options_without_a_merge_interval() {
        let layout = ColumnstoreLayout {
            segment_by: column("stream_key"),
            order_by: vec![
                (column("recorded_at"), SortDirection::Descending),
                (column("write_id"), SortDirection::Ascending),
            ],
            sparse_indexes: vec![
                (SparseIndexKind::Minmax, vec![column("observed_at")]),
                (SparseIndexKind::Bloom, vec![column("recorded_at"), column("write_id")]),
            ],
        };
        pretty_assertions::assert_eq!(
            EnableColumnstore::new(TableName::from(Alias::new("attribute_history")), layout).rendered(),
            r#"ALTER TABLE "attribute_history" SET (timescaledb.enable_columnstore, timescaledb.segmentby = 'stream_key', timescaledb.orderby = 'recorded_at DESC, write_id', timescaledb.sparse_index = 'minmax(observed_at), bloom(recorded_at, write_id)')"#
        );
    }
}
