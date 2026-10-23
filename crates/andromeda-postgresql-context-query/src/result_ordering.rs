//! The `ORDER BY` clause of an entity query (clause 4.23).
//!
//! One `orderBy` term contributes several sort keys, because clause 4.23.2
//! settles the comparison between values of different datatypes before the
//! values themselves are compared, and a distance term settles whether the
//! attribute is a `GeoProperty` at all before that. The keys are emitted in the
//! order the clause reads them: whether a geometry was measured, the measured
//! distance, the datatype rank, then the value.
//!
//! The keys are handed back as values rather than written into the statement,
//! because the paginated entity query orders twice: its page is ordered on the
//! expressions, and the statement that reads the page's heads restates the
//! same order on the columns the page projected them under.

use crate::{attribute_value_order::AttributeValue, order_geography::geo_order_distance};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    error::QueryError,
    ordering::{
        compiled::ResultOrdering,
        sort_direction::SortDirection,
        term::{CompiledOrderTerm, OrderTarget},
    },
};
use andromeda_postgresql_encoding::ewkb::encode;
use andromeda_postgresql_query::{
    dictionary::{names_in_key_order::type_names_of, resolved_names::ResolvedNames},
    entity_ordering::{entity_identifier_key, ordering_collation},
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    sort_key::{KeyOrder, NullPlacement, SortKey},
};
use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use andromeda_postgresql_seaquery::collation::{PostgresCollation, collate};
use sea_query::{Expr as SeaExpr, JoinType, SelectStatement, SimpleExpr};

/// The sort keys of the result order, in the order they decide; the
/// value lookups they read are joined into `select` as laterals.
///
/// Always ends with `id ASC`, even when no `orderBy` terms were supplied, so
/// pagination is stable: clause 5.5.9 requires a consistent order across
/// pages, and `LIMIT`/`OFFSET` over an otherwise unordered result can repeat
/// and skip rows between pages.
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] when a reference geometry
/// has no EWKB spelling the store can bind, and
/// [`RenderError::Query`] carrying [`QueryError::DistanceOrderWithoutReference`]
/// for a distance term with no `orderFrom` to measure from, which Table
/// 6.4.3.2-1 makes mandatory for a distance ordering.
pub fn sort_keys(
    select: &mut SelectStatement,
    rows: &EntityRowRefs,
    ordering: &ResultOrdering,
    attribute_names: &ResolvedNames<AttributeName>,
) -> Result<Vec<SortKey>, RenderError> {
    let mut keys = Vec::new();
    for (position, term) in ordering.terms.iter().enumerate() {
        let direction = SortDirection::from(term.direction);
        let collation = ordering_collation(term.collation.as_ref());
        // A distance ordering with no `orderFrom` is refused whatever the term
        // names: the reference is a property of the request, not of the
        // attribute it happens to address.
        let reference = distance_reference(term, ordering)?;
        match &term.target {
            OrderTarget::Id => keys.push(entity_identifier_key(rows.identity_column(Entities::Id), direction, &collation)),
            // The interned keys sort by dictionary id, which is meaningless to a
            // client; the IRIs are read back through the dictionary and sorted.
            OrderTarget::Type => {
                let names = type_names_of(rows.search_column(EntitySearch::EntityTypeNameKeys));
                keys.push(SortKey::new(collate(names, &collation), nulls_last(direction)));
            }
            // The scope path column is an `ltree[]`, which no client collation
            // applies to. An unscoped entity stores NULL, which is the clause
            // 4.23.2 "Attribute does not exist" and so ranks last.
            OrderTarget::Scope => keys.push(SortKey::new(rows.search_column(EntitySearch::ScopePaths), nulls_last(direction))),
            // Clause 4.8 `createdAt` and `modifiedAt` are `timestamptz` columns
            // of the identity row that are never null, so they need no `NULLS
            // LAST` and no collation; two entities written in one transaction
            // share them and fall to the `id ASC` tie-break below (clause
            // 5.5.9).
            OrderTarget::CreatedAt => keys.push(SortKey::new(
                rows.identity_column(Entities::CreatedAt),
                KeyOrder::new(direction, NullPlacement::Unstated),
            )),
            OrderTarget::ModifiedAt => keys.push(SortKey::new(
                rows.identity_column(Entities::ModifiedAt),
                KeyOrder::new(direction, NullPlacement::Unstated),
            )),
            OrderTarget::Attribute(path) => {
                // Only an attribute is a `GeoProperty`. Clause 4.23.1 sorts by
                // distance when "Attribute is a GeoProperty"; a path descending
                // into a sub-attribute or into a member of a compound value
                // addresses a value held inside one, which has no geometry of
                // its own, so such a term contributes the clause 4.23.2 value
                // keys alone.
                let distance = reference
                    .as_deref()
                    .filter(|_| path.is_simple())
                    .and_then(|reference| geo_order_distance(&path.attr, attribute_names, rows, reference));
                let value = AttributeValue::at(position);
                select.join_lateral(
                    JoinType::LeftJoin,
                    value.lateral(rows, path, attribute_names),
                    value.alias(),
                    SeaExpr::val(true),
                );
                keys.extend(attribute_keys(&value, direction, &collation, distance));
            }
        }
    }

    keys.push(identifier_tiebreak(rows));
    Ok(keys)
}

/// The `id ASC` key every result order ends with, so that pagination is
/// stable (clause 5.5.9).
pub(crate) fn identifier_tiebreak(rows: &EntityRowRefs) -> SortKey {
    SortKey::new(
        rows.identity_column(Entities::Id),
        KeyOrder::new(SortDirection::Ascending, NullPlacement::Unstated),
    )
}

/// What one attribute-valued term compares an Entity on, in the order clause
/// 4.23.2 reads it: the rank of the value's datatype, then the value as a
/// number, as an instant and as collated text, each `NULL` for a value of
/// another datatype.
pub(crate) struct RankedValue {
    /// The clause 4.23.2 rank of the datatype, `NULL` for no value.
    pub(crate) rank: SimpleExpr,
    /// The value when it is a number.
    pub(crate) number: SimpleExpr,
    /// The value when it is a `DateTime`.
    pub(crate) instant: SimpleExpr,
    /// The value when it compares as text, collated.
    pub(crate) text: SimpleExpr,
}

/// The four keys `value` is ordered by in `direction`.
///
/// The rank runs ascending whichever way the term does: the sequence it is
/// read from already runs the term's way, and reversing the key as well would
/// move Null and the absent attribute to the front of a descending sort, which
/// clause 4.23.2 pins to the back.
pub(crate) fn ranked_keys(value: RankedValue, direction: SortDirection) -> [SortKey; 4] {
    let by_value = KeyOrder::new(direction, NullPlacement::Unstated);
    [
        SortKey::new(value.rank, nulls_last(SortDirection::Ascending)),
        SortKey::new(value.number, by_value),
        SortKey::new(value.instant, by_value),
        SortKey::new(value.text, by_value),
    ]
}

/// `direction` with absent values last in either direction (clause 4.23.2).
const fn nulls_last(direction: SortDirection) -> KeyOrder {
    KeyOrder::new(direction, NullPlacement::Last)
}

/// The reference geometry a distance term measures from, encoded as EWKB.
///
/// Only `orderFrom` names it (Table 6.4.3.2-1); a geo-query's geometry decides
/// which entities match, never where distance is measured from. A term that
/// does not sort by distance needs none.
fn distance_reference(term: &CompiledOrderTerm, ordering: &ResultOrdering) -> Result<Option<Vec<u8>>, RenderError> {
    if !term.direction.is_distance() {
        return Ok(None);
    }
    match &ordering.geometry {
        Some(reference) => Ok(Some(encode(reference.geometry.form())?.to_vec())),
        None => Err(RenderError::Query(QueryError::DistanceOrderWithoutReference)),
    }
}

/// The keys one attribute-valued term contributes, read off the value
/// lookup the caller joined.
///
/// A distance term measures first and ranks the entities it could not measure
/// after the ones it could: clause 4.23.2 puts "Attribute is a `GeoProperty`"
/// ahead of "Attribute is not a `GeoProperty` - sorted by value as shown
/// above", in both directions, which is what `NULLS LAST` on the distance says.
/// The clause-4.23.2 keys then follow for every term, distance or not, because
/// they are what "sorted by value" means and because they break a tie between
/// two entities the same distance away.
fn attribute_keys(
    value: &AttributeValue,
    direction: SortDirection,
    collation: &PostgresCollation,
    distance: Option<SimpleExpr>,
) -> impl Iterator<Item = SortKey> {
    let measured = distance.map(|distance| SortKey::new(distance, nulls_last(direction)));
    let ranked = ranked_keys(
        RankedValue {
            rank: value.datatype_rank(direction),
            number: value.numeric_key(),
            instant: value.instant_key(),
            text: value.text_key(collation),
        },
        direction,
    );
    measured.into_iter().chain(ranked)
}

#[cfg(test)]
mod tests {
    use crate::result_ordering::{RankedValue, ranked_keys};
    use andromeda_ngsi_ld_query::ordering::sort_direction::SortDirection;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    fn ordered(direction: SortDirection) -> String {
        let column = |name: &str| Expr::col(Alias::new(name));
        let value = RankedValue {
            rank: column("r"),
            number: column("n"),
            instant: column("i"),
            text: column("t"),
        };
        let mut select = Query::select();
        select.expr(column("id"));
        for key in ranked_keys(value, direction) {
            key.order_by(&mut select);
        }
        select.to_string(PostgresQueryBuilder)
    }

    /// Clause 4.23.2 pins Null and the absent Attribute last, so the rank
    /// runs ascending in both directions while the values follow the term.
    #[test]
    fn the_rank_runs_ascending_and_the_values_run_the_terms_way() {
        assert_eq!(
            ordered(SortDirection::Ascending),
            r#"SELECT "id" ORDER BY "r" ASC NULLS LAST, "n" ASC, "i" ASC, "t" ASC"#
        );
        assert_eq!(
            ordered(SortDirection::Descending),
            r#"SELECT "id" ORDER BY "r" ASC NULLS LAST, "n" DESC, "i" DESC, "t" DESC"#
        );
    }
}
