//! The temporal history hypertables whose chunks the columnstore converts and
//! whose closed rowstore chunks have their indexes rebuilt.

use andromeda_postgresql_schema::temporal::{RebuiltHistoryChunks, RowstoreHistoryChunks};
use andromeda_postgresql_seaquery::columnstore_chunk::ListedChunks;
use sea_query::IntoIden;
use strum::{Display, EnumString, IntoStaticStr};

/// A temporal history hypertable whose chunks the columnstore converts.
///
/// The variant names render, in `snake_case`, to the exact `public` hypertable
/// identifiers tenant migrations create, so the same value is both the SQL
/// filter term and the metric label. Scope history is a plain table: it holds
/// one row per change of Scope per Entity and gains nothing from chunking.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum HistoryTable {
    /// Attribute instance history.
    AttributeHistory,
}

impl HistoryTable {
    /// Every history hypertable.
    pub const ALL: [HistoryTable; 1] = [HistoryTable::AttributeHistory];

    /// The exact hypertable identifier created by tenant migrations.
    #[must_use]
    pub const fn relation_name(self) -> &'static str {
        match self {
            HistoryTable::AttributeHistory => "attribute_history",
        }
    }

    /// The relation listing the chunks the broker's conversion sweep keeps in
    /// the rowstore for good, which are neither overdue nor ever converted.
    #[must_use]
    pub fn kept_chunks(self) -> ListedChunks {
        match self {
            HistoryTable::AttributeHistory => ListedChunks {
                table: RowstoreHistoryChunks::Table.into_iden(),
                range_start: RowstoreHistoryChunks::RangeStart.into_iden(),
            },
        }
    }

    /// The relation listing the closed rowstore chunks whose indexes
    /// maintenance has rebuilt.
    #[must_use]
    pub fn rebuilt_chunks(self) -> ListedChunks {
        match self {
            HistoryTable::AttributeHistory => ListedChunks {
                table: RebuiltHistoryChunks::Table.into_iden(),
                range_start: RebuiltHistoryChunks::RangeStart.into_iden(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::candidate::history_table::HistoryTable;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn each_history_table_name_round_trips_and_is_the_hypertable_identifier() {
        for table in HistoryTable::ALL {
            assert_eq!(HistoryTable::from_str(table.relation_name()), Ok(table));
            assert_eq!(<&'static str>::from(table), table.relation_name());
        }
        assert_eq!(HistoryTable::AttributeHistory.relation_name(), "attribute_history");
        assert!(HistoryTable::from_str("scope_history").is_err());
    }
}
