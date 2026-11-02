//! The types whose Temporal Evolutions hold streams of the attributes a
//! temporal selection names, read from the stream type census.
//!
//! Clause 5.7.4.4 S1 selects the Evolutions holding an instance of an
//! attribute the request names, and clause 5.5.9 pages them in identifier
//! order. No stream carries the Entity's identifier, so the holders are paged
//! through the Evolution memberships of the types holding the attribute,
//! merged in identifier order: a membership range is ordered by identifier
//! already. The census names those types and how many streams each holds,
//! read once before the statement is rendered, as the dictionaries are.

use crate::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
use andromeda_postgresql_schema::temporal::StreamTypeCensus;
use andromeda_postgresql_seaquery::{branch::qualified, cast::cast, literal::int_literal, pg_array::is_any_of, pg_type::PostgresType};
use sea_query::{DynIden, Expr, ExprTrait, Func, Iden, Query, SelectStatement, SimpleExpr};
use std::collections::HashMap;

/// The number of streams the census reads under each type, `streams`.
#[derive(Iden, Clone, Copy)]
#[iden = "streams"]
pub struct HeldStreams;

/// One type whose Temporal Evolutions hold streams of an attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HolderType {
    /// The type's dictionary key.
    pub type_key: EntityTypeNameKey,
    /// How many streams of the attribute its Evolutions hold.
    pub streams: u64,
}

/// The holding types of every attribute a selection names; an attribute it
/// holds no type for is held by no recorded Evolution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributeHolders {
    /// The holding types of each attribute, in type key order.
    by_attribute: HashMap<AttributeNameKey, Vec<HolderType>>,
}

impl AttributeHolders {
    /// The holders of the census rows `rows` reads, each an attribute, a type
    /// and the number of streams.
    #[must_use]
    pub fn new(rows: impl IntoIterator<Item = (AttributeNameKey, HolderType)>) -> AttributeHolders {
        let mut by_attribute: HashMap<AttributeNameKey, Vec<HolderType>> = HashMap::new();
        for (attribute, holder) in rows {
            by_attribute.entry(attribute).or_default().push(holder);
        }
        for holders in by_attribute.values_mut() {
            holders.sort_unstable_by_key(|holder| holder.type_key.get());
        }
        AttributeHolders { by_attribute }
    }

    /// The types holding streams of `attribute`.
    #[must_use]
    pub fn of(&self, attribute: AttributeNameKey) -> &[HolderType] {
        self.by_attribute.get(&attribute).map_or(&[], Vec::as_slice)
    }

    /// How many streams of `attribute` the census counts, a stream counting
    /// once under each type of its Evolution: never fewer than the attribute's
    /// streams.
    #[must_use]
    pub fn streams_of(&self, attribute: AttributeNameKey) -> u64 {
        self.of(attribute).iter().map(|holder| holder.streams).sum()
    }
}

/// The holding types of the attributes keyed `keys` in the census of the
/// branch `schema` names, with the number of streams under each:
///
/// ```sql
/// SELECT attribute_name_key, entity_type_name_key, SUM(stream_delta)::bigint AS streams
/// FROM stream_type_census WHERE attribute_name_key = ANY(keys)
/// GROUP BY attribute_name_key, entity_type_name_key HAVING SUM(stream_delta) > 0
/// ```
#[must_use]
pub fn holder_types_query(schema: Option<&DynIden>, keys: &[AttributeNameKey]) -> SelectStatement {
    let streams = || -> SimpleExpr { Func::sum(Expr::col(StreamTypeCensus::StreamDelta)).into() };
    Query::select()
        .column(StreamTypeCensus::AttributeNameKey)
        .column(StreamTypeCensus::EntityTypeNameKey)
        .expr_as(cast(streams(), PostgresType::Bigint), HeldStreams)
        .from(qualified(schema, StreamTypeCensus::Table))
        .and_where(is_any_of(Expr::col(StreamTypeCensus::AttributeNameKey), keys.iter().map(|key| key.get())))
        .group_by_col(StreamTypeCensus::AttributeNameKey)
        .group_by_col(StreamTypeCensus::EntityTypeNameKey)
        .and_having(streams().gt(int_literal(0)))
        .take()
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        entity_type_name_key::EntityTypeNameKey,
        history_source::attribute_holders::{AttributeHolders, HolderType, holder_types_query},
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder};

    fn holder(type_key: i32, streams: u64) -> HolderType {
        HolderType {
            type_key: EntityTypeNameKey::from(type_key),
            streams,
        }
    }

    #[test]
    fn the_census_is_read_as_the_positive_sums_of_the_named_attributes() {
        assert_eq!(
            holder_types_query(None, &[AttributeNameKey::from(7)]).to_string(PostgresQueryBuilder),
            concat!(
                r#"SELECT "attribute_name_key", "entity_type_name_key", SUM("stream_delta")::bigint AS "streams" FROM "stream_type_census" "#,
                r#"WHERE "attribute_name_key" = 7 GROUP BY "attribute_name_key", "entity_type_name_key" HAVING SUM("stream_delta") > (0)"#
            )
        );
    }

    #[test]
    fn a_snapshot_branch_reads_its_own_census() {
        let schema = Alias::new("snap_2").into_iden();
        let sql = holder_types_query(Some(&schema), &[AttributeNameKey::from(7)]).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "snap_2"."stream_type_census""#), "{sql}");
    }

    #[test]
    fn each_attribute_keeps_its_own_holding_types_in_key_order() {
        let speed = AttributeNameKey::from(7);
        let holders = AttributeHolders::new([(speed, holder(5, 10)), (AttributeNameKey::from(8), holder(4, 1)), (speed, holder(3, 2))]);
        assert_eq!(holders.of(speed), [holder(3, 2), holder(5, 10)]);
        assert_eq!(holders.streams_of(speed), 12);
        assert_eq!(holders.of(AttributeNameKey::from(9)), []);
        assert_eq!(holders.streams_of(AttributeNameKey::from(9)), 0);
    }
}
