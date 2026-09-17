//! The `ORDER BY` clause of an entity query (clause 4.23).
//!
//! One `orderBy` term contributes several sort keys, because clause 4.23.2
//! settles the comparison between values of different datatypes before the
//! values themselves are compared, and a distance term settles whether the
//! attribute is a `GeoProperty` at all before that. The keys are emitted in the
//! order the clause reads them: whether a geometry was measured, the measured
//! distance, the datatype rank, then the value.

use crate::{attribute_value_order::AttributeValue, order_geography::geo_order_distance};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    error::QueryError,
    filter::path::AttrPath,
    geo::query::GeoQuery,
    ordering::{
        compiled::ResultOrdering,
        sort_direction::SortDirection,
        term::{CompiledOrderTerm, OrderTarget},
    },
};
use andromeda_postgresql_encoding::ewkb::encode;
use andromeda_postgresql_query::{
    dictionary::resolved_names::ResolvedNames,
    entity_ordering::{order_by_entity_identifier, ordering_collation, sql_order},
    entity_row_refs::EntityRowRefs,
    entity_type_names::type_names_of,
    error::RenderError,
};
use andromeda_postgresql_schema::current::{Entities, EntityDocuments, EntitySearch};
use andromeda_postgresql_seaquery::collation::{PostgresCollation, collate};
use sea_query::{Expr as SeaExpr, JoinType, NullOrdering, Order, SelectStatement, SimpleExpr};

/// Add the `ORDER BY` clause, and the projections its keys read, to `select`.
///
/// Always terminates the ordering with `id ASC`, even when no `orderBy` terms
/// were supplied, so pagination is stable: clause 5.5.9 requires a consistent
/// order across pages, and `LIMIT`/`OFFSET` over an otherwise unordered result
/// can repeat and skip rows between pages.
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] when a reference geometry
/// has no EWKB spelling the store can bind, and
/// [`RenderError::Query`] carrying [`QueryError::DistanceOrderWithoutReference`]
/// for a distance term with no reference geometry anywhere in the query to
/// measure from, which clause 5.7.2.4 makes `BadRequestData`.
pub fn apply(
    select: &mut SelectStatement,
    rows: &EntityRowRefs,
    ordering: &ResultOrdering,
    geo: Option<&GeoQuery>,
    attribute_names: &ResolvedNames<AttributeName>,
) -> Result<(), RenderError> {
    for (position, term) in ordering.terms.iter().enumerate() {
        let direction = SortDirection::from(term.direction);
        let collation = ordering_collation(term.collation.as_ref());
        // Clause 5.7.2.4 rejects a distance ordering with no location to
        // measure from, whatever the term names: the reference is a property of
        // the request, not of the attribute it happens to address.
        let reference = distance_reference(term, ordering, geo)?;
        match &term.target {
            OrderTarget::Id => order_by_entity_identifier(select, rows.identity_column(Entities::Id), direction, &collation),
            // The interned keys sort by dictionary id, which is meaningless to a
            // client; the IRIs are read back through the dictionary and sorted.
            OrderTarget::Type => {
                let names = type_names_of(rows.search_column(EntitySearch::TypeIds));
                select.order_by_expr_with_nulls(collate(names, &collation), sql_order(direction), NullOrdering::Last);
            }
            // The scope path column is an `ltree[]`, which no client collation applies to.
            OrderTarget::Scope => {
                select.order_by_expr_with_nulls(rows.search_column(EntitySearch::ScopePaths), sql_order(direction), NullOrdering::Last);
            }
            // Clause 4.8 `createdAt` is a `timestamptz` on the identity row that
            // is never null, so it needs no `NULLS LAST` and no collation; two
            // entities created in one transaction share it and fall to the
            // `id ASC` tie-break below (clause 5.5.9).
            OrderTarget::CreatedAt => {
                select.order_by_expr(rows.identity_column(Entities::CreatedAt), sql_order(direction));
            }
            // Clause 4.8 `modifiedAt` lives on `entity_documents`; it is
            // nullable, so absent values sort last in both directions
            // (clause 4.23.2).
            OrderTarget::ModifiedAt => {
                select.order_by_expr_with_nulls(rows.documents_column(EntityDocuments::ModifiedAt), sql_order(direction), NullOrdering::Last);
            }
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
                order_by_attribute(select, &rows.payload_column(), path, position, direction, &collation, distance);
            }
        }
    }

    // Always add id ASC as tiebreaker
    select.order_by((Entities::Table, Entities::Id), Order::Asc);

    Ok(())
}

/// The reference geometry a distance term measures from, encoded as EWKB.
///
/// `orderFrom` names it outright (clause 5.2.43); a query that only carries a
/// geo-query measures from the geometry that query already matched against.
/// A term that does not sort by distance needs none.
fn distance_reference(term: &CompiledOrderTerm, ordering: &ResultOrdering, geo: Option<&GeoQuery>) -> Result<Option<Vec<u8>>, RenderError> {
    if !term.direction.is_distance() {
        return Ok(None);
    }
    match (ordering.geometry.as_ref(), geo) {
        (Some(reference), _) => Ok(Some(encode(&reference.geometry)?.to_vec())),
        (None, Some(geo)) => Ok(Some(encode(&geo.geometry)?.to_vec())),
        (None, None) => Err(RenderError::Query(QueryError::DistanceOrderWithoutReference)),
    }
}

/// Add the keys one attribute-valued term contributes.
///
/// A distance term measures first and ranks the entities it could not measure
/// after the ones it could: clause 4.23.2 puts "Attribute is a `GeoProperty`"
/// ahead of "Attribute is not a `GeoProperty` - sorted by value as shown
/// above", in both directions, which is what `NULLS LAST` on the distance says.
/// The clause-4.23.2 keys then follow for every term, distance or not, because
/// they are what "sorted by value" means and because they break a tie between
/// two entities the same distance away.
fn order_by_attribute(
    select: &mut SelectStatement,
    payload: &SimpleExpr,
    path: &AttrPath,
    position: usize,
    direction: SortDirection,
    collation: &PostgresCollation,
    distance: Option<SimpleExpr>,
) {
    if let Some(distance) = distance {
        select.order_by_expr_with_nulls(distance, sql_order(direction), NullOrdering::Last);
    }

    let value = AttributeValue::at(position);
    select.join_lateral(JoinType::LeftJoin, value.lateral(payload, path), value.alias(), SeaExpr::val(true));
    // The rank runs ascending whichever way the term does: the sequence it is
    // read from already runs the term's way, and reversing the key as well
    // would move Null and the absent attribute to the front of a descending
    // sort, which clause 4.23.2 pins to the back.
    select.order_by_expr_with_nulls(value.datatype_rank(direction), Order::Asc, NullOrdering::Last);
    select.order_by_expr(value.numeric_key(), sql_order(direction));
    select.order_by_expr(value.instant_key(), sql_order(direction));
    select.order_by_expr(value.text_key(collation), sql_order(direction));
}
