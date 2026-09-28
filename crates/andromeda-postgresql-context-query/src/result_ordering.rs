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
//! expressions, and the statement that reads the page's payloads restates the
//! same order on the columns the page projected them under.

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
    dictionary::{names_in_key_order::type_names_of, resolved_names::ResolvedNames},
    entity_ordering::{entity_identifier_key, ordering_collation},
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    sort_key::{KeyOrder, NullPlacement, SortKey},
};
use andromeda_postgresql_schema::current::{Entities, EntityDocuments, EntitySearch};
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
/// for a distance term with no reference geometry anywhere in the query to
/// measure from, which clause 5.7.2.4 makes `BadRequestData`.
pub fn sort_keys(
    select: &mut SelectStatement,
    rows: &EntityRowRefs,
    ordering: &ResultOrdering,
    geo: Option<&GeoQuery>,
    attribute_names: &ResolvedNames<AttributeName>,
) -> Result<Vec<SortKey>, RenderError> {
    let mut keys = Vec::new();
    for (position, term) in ordering.terms.iter().enumerate() {
        let direction = SortDirection::from(term.direction);
        let collation = ordering_collation(term.collation.as_ref());
        // Clause 5.7.2.4 rejects a distance ordering with no location to
        // measure from, whatever the term names: the reference is a property of
        // the request, not of the attribute it happens to address.
        let reference = distance_reference(term, ordering, geo)?;
        match &term.target {
            OrderTarget::Id => keys.push(entity_identifier_key(rows.identity_column(Entities::Id), direction, &collation)),
            // The interned keys sort by dictionary id, which is meaningless to a
            // client; the IRIs are read back through the dictionary and sorted.
            OrderTarget::Type => {
                let names = type_names_of(rows.search_column(EntitySearch::EntityTypeNameKeys));
                keys.push(SortKey::new(collate(names, &collation), nulls_last(direction)));
            }
            // The scope path column is an `ltree[]`, which no client collation applies to.
            OrderTarget::Scope => keys.push(SortKey::new(rows.search_column(EntitySearch::ScopePaths), nulls_last(direction))),
            // Clause 4.8 `createdAt` is a `timestamptz` on the identity row that
            // is never null, so it needs no `NULLS LAST` and no collation; two
            // entities created in one transaction share it and fall to the
            // `id ASC` tie-break below (clause 5.5.9).
            OrderTarget::CreatedAt => keys.push(SortKey::new(
                rows.identity_column(Entities::CreatedAt),
                KeyOrder::new(direction, NullPlacement::Unstated),
            )),
            // Clause 4.8 `modifiedAt` lives on `entity_documents`; it is
            // nullable, so absent values sort last in both directions
            // (clause 4.23.2).
            OrderTarget::ModifiedAt => keys.push(SortKey::new(rows.documents_column(EntityDocuments::ModifiedAt), nulls_last(direction))),
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
                keys.extend(attribute_keys(select, &rows.payload_column(), path, position, direction, &collation, distance));
            }
        }
    }

    keys.push(SortKey::new(
        rows.identity_column(Entities::Id),
        KeyOrder::new(SortDirection::Ascending, NullPlacement::Unstated),
    ));
    Ok(keys)
}

/// `direction` with absent values last in either direction (clause 4.23.2).
const fn nulls_last(direction: SortDirection) -> KeyOrder {
    KeyOrder::new(direction, NullPlacement::Last)
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

/// The keys one attribute-valued term contributes, with the value lookup they
/// read joined into `select`.
///
/// A distance term measures first and ranks the entities it could not measure
/// after the ones it could: clause 4.23.2 puts "Attribute is a `GeoProperty`"
/// ahead of "Attribute is not a `GeoProperty` - sorted by value as shown
/// above", in both directions, which is what `NULLS LAST` on the distance says.
/// The clause-4.23.2 keys then follow for every term, distance or not, because
/// they are what "sorted by value" means and because they break a tie between
/// two entities the same distance away.
fn attribute_keys(
    select: &mut SelectStatement,
    payload: &SimpleExpr,
    path: &AttrPath,
    position: usize,
    direction: SortDirection,
    collation: &PostgresCollation,
    distance: Option<SimpleExpr>,
) -> impl Iterator<Item = SortKey> {
    let value = AttributeValue::at(position);
    select.join_lateral(JoinType::LeftJoin, value.lateral(payload, path), value.alias(), SeaExpr::val(true));
    let by_value = KeyOrder::new(direction, NullPlacement::Unstated);
    let measured = distance.map(|distance| SortKey::new(distance, nulls_last(direction)));
    // The rank runs ascending whichever way the term does: the sequence it is
    // read from already runs the term's way, and reversing the key as well
    // would move Null and the absent attribute to the front of a descending
    // sort, which clause 4.23.2 pins to the back.
    let ranked = [
        SortKey::new(value.datatype_rank(direction), nulls_last(SortDirection::Ascending)),
        SortKey::new(value.numeric_key(), by_value),
        SortKey::new(value.instant_key(), by_value),
        SortKey::new(value.text_key(collation), by_value),
    ];
    measured.into_iter().chain(ranked)
}
