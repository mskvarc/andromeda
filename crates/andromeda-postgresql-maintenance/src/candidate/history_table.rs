//! The temporal history hypertables whose chunks the columnstore converts.

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
