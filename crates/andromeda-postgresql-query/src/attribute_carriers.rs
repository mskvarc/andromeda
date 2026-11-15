//! How many instances of an attribute the census counts across every Entity,
//! read before a statement is rendered when the choice of its keyed conjunct
//! depends on it.
//!
//! A conjunct's candidates are Entities holding an instance of the attribute
//! its index is read through, so the census count of that attribute bounds
//! them from above. The count is read once per query, as the dictionaries
//! are, and only for a filter with two or more conjuncts an index answers.

use andromeda_postgresql_schema::census::AttributeCensus;
use andromeda_postgresql_seaquery::{branch::qualified, cast::cast, pg_array::is_any_of, pg_type::PostgresType};
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
use sea_query::{DynIden, Expr, Func, Iden, Query, SelectStatement, SimpleExpr};
use std::collections::HashMap;

/// The instance count the census read names under each attribute, `instances`.
#[derive(Iden, Clone, Copy)]
#[iden = "instances"]
pub struct CountedInstances;

/// The census counts of the attributes a filter's keyed conjuncts are read
/// through; an attribute the read did not cover has no count.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributeCarriers {
    /// The instances of each attribute the census counts.
    by_attribute: HashMap<AttributeNameKey, u64>,
}

impl AttributeCarriers {
    /// The counts of the attributes `read` that the census rows `rows` give,
    /// each an attribute and its instances: an attribute read with no row has
    /// no instance.
    #[must_use]
    pub fn new(read: &[AttributeNameKey], rows: impl IntoIterator<Item = (AttributeNameKey, u64)>) -> AttributeCarriers {
        let mut by_attribute: HashMap<AttributeNameKey, u64> = read.iter().map(|key| (*key, 0)).collect();
        by_attribute.extend(rows);
        AttributeCarriers { by_attribute }
    }

    /// The instances of `attribute` the census counts: zero for an attribute
    /// the read covered and found no instance of, `None` for one it did not
    /// cover.
    #[must_use]
    pub fn instances(&self, attribute: AttributeNameKey) -> Option<u64> {
        self.by_attribute.get(&attribute).copied()
    }
}

/// The instances of each attribute keyed `keys` in the census of the branch
/// `schema` names:
///
/// ```sql
/// SELECT attribute_name_key, SUM(instance_delta)::bigint AS instances
/// FROM attribute_census WHERE attribute_name_key = ANY(keys)
/// GROUP BY attribute_name_key
/// ```
///
/// An attribute with no ledger row has no row in the answer, which counts as
/// no instance.
#[must_use]
pub fn carriers_query(schema: Option<&DynIden>, keys: &[AttributeNameKey]) -> SelectStatement {
    let instances = || -> SimpleExpr { Func::sum(Expr::col(AttributeCensus::InstanceDelta)).into() };
    Query::select()
        .column(AttributeCensus::AttributeNameKey)
        .expr_as(cast(instances(), PostgresType::Bigint), CountedInstances)
        .from(qualified(schema, AttributeCensus::Table))
        .and_where(is_any_of(Expr::col(AttributeCensus::AttributeNameKey), keys.iter().map(|key| key.get())))
        .group_by_col(AttributeCensus::AttributeNameKey)
        .take()
}

#[cfg(test)]
mod tests {
    use crate::attribute_carriers::{AttributeCarriers, carriers_query};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, DynIden, IntoIden};

    #[test]
    fn the_census_read_sums_the_ledger_of_each_attribute() {
        assert_statement_snapshot!(
            "attribute-carriers-the-census-read-sums-the-ledger-of-each-attribute",
            carriers_query(None, &[AttributeNameKey::from(4), AttributeNameKey::from(9)])
        );
    }

    #[test]
    fn a_snapshot_reads_the_census_of_its_branch() {
        let schema: DynIden = Alias::new("snapshot_7_published").into_iden();
        assert_statement_snapshot!(
            "attribute-carriers-a-snapshot-reads-the-census-of-its-branch",
            carriers_query(Some(&schema), &[AttributeNameKey::from(4)])
        );
    }

    #[test]
    fn an_attribute_read_without_a_row_has_no_instance_and_one_not_read_has_no_count() {
        let carriers = AttributeCarriers::new(&[AttributeNameKey::from(4), AttributeNameKey::from(6)], [(AttributeNameKey::from(4), 12)]);
        assert_eq!(carriers.instances(AttributeNameKey::from(4)), Some(12));
        assert_eq!(carriers.instances(AttributeNameKey::from(6)), Some(0), "read and found no instance");
        assert_eq!(carriers.instances(AttributeNameKey::from(5)), None);
    }
}
