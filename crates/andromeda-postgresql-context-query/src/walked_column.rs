//! The value columns a page can walk in value order, and what a walk of each
//! reads and projects.
//!
//! Clause 4.23.1 makes the order of numbers and of `DateTime` values trivial: a
//! number compares as itself and a `DateTime` as its instant, which is what
//! `value_number` and `value_date_time` hold and their attribute-led B-trees
//! order. Every value in one of those columns ranks as one clause 4.23.2
//! datatype, so an Attribute whose instances occupy that column alone orders by
//! the column, and a walk projects the column's value as the one key of its
//! datatype, every other key `NULL`.

use crate::{
    attribute_value_datatype::{NUMBER_TYPE, datatype_rank},
    occupied_columns::HeldColumn,
    result_ordering::RankedValue,
};
use andromeda_ngsi_ld_query::ordering::sort_direction::SortDirection;
use andromeda_ngsi_ld_vocabulary::expanded::classes::DATE_TIME;
use andromeda_postgresql_query::q_predicate::projection_probe::projection_column;
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::{
    cast::typed_null,
    collation::{PostgresCollation, collate},
    literal::text_literal,
    pg_type::PostgresType,
};
use sea_query::Iden;

/// A value column a page walks in the order of its value index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WalkedColumn {
    /// `value_number`, every value a Number.
    Number,
    /// `value_date_time`, every value a `DateTime`.
    DateTime,
}

/// The CTE holding the carriers one walk read.
#[derive(Iden, Clone, Copy)]
pub(crate) enum WalkedCarriers {
    /// The carriers of a number walk.
    #[iden = "walked_numbers"]
    Numbers,
    /// The carriers of a `DateTime` walk.
    #[iden = "walked_instants"]
    Instants,
}

impl WalkedColumn {
    /// Every walked column, in the order the statement holds their walks.
    pub(crate) const ALL: [WalkedColumn; 2] = [WalkedColumn::Number, WalkedColumn::DateTime];

    /// The column the walk reads.
    pub(crate) const fn held(self) -> HeldColumn {
        match self {
            WalkedColumn::Number => HeldColumn::Number,
            WalkedColumn::DateTime => HeldColumn::DateTime,
        }
    }

    /// The CTE the walk's carriers are read into.
    pub(crate) const fn carriers(self) -> WalkedCarriers {
        match self {
            WalkedColumn::Number => WalkedCarriers::Numbers,
            WalkedColumn::DateTime => WalkedCarriers::Instants,
        }
    }

    /// A carrier's value, under the keys the plain page orders by: the rank
    /// of the column's datatype in `direction`, then the column as the key of
    /// that datatype, the other keys `NULL` and the text key collated as the
    /// plain page's.
    pub(crate) fn ranked_value(self, direction: SortDirection, collation: &PostgresCollation) -> RankedValue {
        let text = collate(typed_null(PostgresType::Text), collation);
        match self {
            WalkedColumn::Number => RankedValue {
                rank: datatype_rank(text_literal(NUMBER_TYPE), direction),
                number: projection_column(CurrentAttributes::ValueNumber),
                instant: typed_null(PostgresType::Timestamptz),
                text,
            },
            WalkedColumn::DateTime => RankedValue {
                rank: datatype_rank(text_literal(DATE_TIME.as_str()), direction),
                number: typed_null(PostgresType::Numeric),
                instant: projection_column(CurrentAttributes::ValueDateTime),
                text,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::walked_column::WalkedColumn;
    use andromeda_ngsi_ld_query::ordering::sort_direction::SortDirection;
    use andromeda_postgresql_query::entity_ordering::ordering_collation;
    use sea_query::{PostgresQueryBuilder, Query};

    fn rendered(column: WalkedColumn, direction: SortDirection) -> String {
        let value = column.ranked_value(direction, &ordering_collation(None));
        Query::select()
            .expr(value.rank)
            .expr(value.number)
            .expr(value.instant)
            .expr(value.text)
            .take()
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_walked_number_projects_its_value_as_the_number_key() {
        let sql = rendered(WalkedColumn::Number, SortDirection::Ascending);
        assert!(
            sql.ends_with(r#"'number'), "ca"."value_number", NULL::timestamptz, (NULL::text) COLLATE "und-x-icu""#),
            "{sql}"
        );
        assert!(sql.starts_with("SELECT array_position("), "{sql}");
    }

    #[test]
    fn a_walked_date_time_projects_its_instant_as_the_instant_key() {
        let sql = rendered(WalkedColumn::DateTime, SortDirection::Ascending);
        assert!(
            sql.ends_with(r#"'https://uri.etsi.org/ngsi-ld/DateTime'), NULL::numeric, "ca"."value_date_time", (NULL::text) COLLATE "und-x-icu""#),
            "{sql}"
        );
        assert!(sql.starts_with("SELECT array_position("), "{sql}");
    }

    /// Clause 4.23.2 reverses the datatype order for a descending term and
    /// keeps Null last.
    #[test]
    fn a_descending_walk_ranks_on_the_reversed_sequence() {
        for column in WalkedColumn::ALL {
            let sql = rendered(column, SortDirection::Descending);
            assert!(
                sql.contains("ARRAY ['https://uri.etsi.org/ngsi-ld/DateTime','https://uri.etsi.org/ngsi-ld/Date',"),
                "{column:?}: {sql}"
            );
            assert!(sql.contains("'string','number','null']::text[]"), "{column:?}: {sql}");
        }
    }
}
