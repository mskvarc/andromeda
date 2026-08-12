//! The geography expression a distance ordering measures to (clause 4.23).

use andromeda_ngsi_ld_query::ordering::term::OrderTarget;
use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
use andromeda_postgresql_query::{
    attribute_instance_correlation::{CorrelatedEntityRow, geometry_instances_of, instance_geometry_column},
    attribute_name_id::AttributeNameId,
    q_predicate::attribute_names::ResolvedAttributeNames,
};

/// SQL expression measuring an entity's distance to the reference geometry a
/// distance ordering sorts by (clause 4.23).
///
/// The returned text carries a single `$1` placeholder, which the caller binds
/// the reference geometry to as EWKB.
///
/// The default GeoProperty has a dedicated geography column; every other one is
/// read from its `current_attribute_values` instance rows, the same projection
/// the geo filter itself reads, so an ordering and a filter can never disagree
/// about where a GeoProperty's geometry lives. Where a GeoProperty has several
/// `datasetId` instances the ordering takes the smallest of their distances,
/// which is the reading that agrees with clause 4.10 counting an entity as
/// matching when any one instance does.
///
/// `outer` names the `entities` row the ordering sorts, because both renderings
/// are read against it: the dedicated column belongs to that row, and the
/// instance subquery correlates back to its surrogate key.
///
/// Clause 4.23's `id` and `type` keywords carry no geometry at all, and neither
/// does a GeoProperty the broker has never recorded, so a distance ordering on
/// either measures from nothing and yields NULL, which sorts every row equally
/// and leaves the `id` tiebreaker to decide.
pub fn geo_order_distance(target: &OrderTarget, attribute_names: &ResolvedAttributeNames, outer: CorrelatedEntityRow<'_>) -> String {
    match order_subject(target, attribute_names) {
        OrderSubject::None => "ST_Distance(NULL::geography, ST_MakeValid(ST_GeomFromEWKB($1))::geography)".to_string(),
        OrderSubject::LocationColumn => format!("ST_Distance({}, ST_MakeValid(ST_GeomFromEWKB($1))::geography)", outer.column("location_geo")),
        OrderSubject::Instances(attribute_id) => format!(
            "(SELECT MIN(ST_Distance({}::geography, ST_MakeValid(ST_GeomFromEWKB($1))::geography)) {})",
            instance_geometry_column(ORDER_ALIAS),
            geometry_instances_of(outer, ORDER_ALIAS, attribute_id),
        ),
    }
}

/// Alias the correlated ordering subquery reads `current_attribute_values`
/// under.
///
/// Deliberately not the alias the geo predicate uses: a query that both filters
/// and orders on GeoProperties nests the two subqueries in one statement.
const ORDER_ALIAS: &str = "geo_order";

/// Where a distance ordering reads the entity's geometry from.
enum OrderSubject {
    /// The ordering term names nothing that carries a geometry.
    None,
    /// The default GeoProperty's dedicated geography column.
    LocationColumn,
    /// The instance rows of one attribute, by dictionary key.
    Instances(AttributeNameId),
}

/// Classify an ordering term by where its geometry is stored.
fn order_subject(target: &OrderTarget, attribute_names: &ResolvedAttributeNames) -> OrderSubject {
    let OrderTarget::Attribute(geoproperty) = target else {
        return OrderSubject::None;
    };
    if geoproperty.as_str() == LOCATION.as_str() {
        return OrderSubject::LocationColumn;
    }
    match attribute_names.get(geoproperty) {
        Some(attribute_id) => OrderSubject::Instances(attribute_id),
        None => OrderSubject::None,
    }
}

#[cfg(test)]
mod tests {
    use crate::order_geography::geo_order_distance;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::ordering::term::OrderTarget;
    use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
    use andromeda_postgresql_query::{
        attribute_instance_correlation::CorrelatedEntityRow,
        attribute_name_id::AttributeNameId,
        q_predicate::attribute_names::ResolvedAttributeNames,
    };

    const CUSTOM_GEOPROPERTY: &str = "https://example.com/customGeo";
    const CUSTOM_GEOPROPERTY_ID: i32 = 42;

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    fn attribute(value: &str) -> OrderTarget {
        OrderTarget::Attribute(name(value))
    }

    fn resolved_custom() -> ResolvedAttributeNames {
        ResolvedAttributeNames::from_pairs([(name(CUSTOM_GEOPROPERTY), AttributeNameId::from(CUSTOM_GEOPROPERTY_ID))])
    }

    /// The entity statement selects from `entities` unaliased.
    fn outer() -> CorrelatedEntityRow<'static> {
        CorrelatedEntityRow::unaliased()
    }

    #[test]
    fn the_default_geoproperty_measures_from_its_dedicated_geography_column() {
        let sql = geo_order_distance(&attribute(LOCATION.as_str()), &ResolvedAttributeNames::default(), outer());
        assert!(sql.contains("ST_Distance(\"entities\".\"location_geo\""), "{sql}");
        assert!(
            !sql.contains("current_attribute_values"),
            "the default GeoProperty needs no instance lookup: {sql}"
        );
    }

    /// Clause 4.23 read together with clause 4.10: an ordering reads the same
    /// projection the filter does, so a named GeoProperty measures from its
    /// instance rows and takes the smallest distance among them.
    #[test]
    fn a_custom_geoproperty_measures_from_its_nearest_attribute_instance() {
        let sql = geo_order_distance(&attribute(CUSTOM_GEOPROPERTY), &resolved_custom(), outer());
        assert!(sql.contains("FROM \"current_attribute_values\""), "{sql}");
        assert!(sql.contains("MIN(ST_Distance("), "the nearest instance decides: {sql}");
        assert!(sql.contains(&format!("\"attribute_name_id\" = {CUSTOM_GEOPROPERTY_ID}")), "{sql}");
        assert!(
            sql.contains("\"geo_order\".\"entity_key\" = \"entities\".\"entity_key\""),
            "correlated to the ordered row: {sql}"
        );
        assert!(!sql.contains("ST_GeomFromGeoJSON"), "the geometry is stored decoded: {sql}");
    }

    /// The correlation binds the outer relation it was handed rather than
    /// whichever `entity_key` column the enclosing statement happens to expose.
    #[test]
    fn the_ordering_subquery_correlates_through_the_outer_row_it_is_given() {
        let sql = geo_order_distance(&attribute(CUSTOM_GEOPROPERTY), &resolved_custom(), CorrelatedEntityRow::aliased("e"));
        assert!(sql.contains("\"geo_order\".\"entity_key\" = \"e\".\"entity_key\""), "{sql}");
        let default_sql = geo_order_distance(
            &attribute(LOCATION.as_str()),
            &ResolvedAttributeNames::default(),
            CorrelatedEntityRow::aliased("e"),
        );
        assert!(default_sql.contains("ST_Distance(\"e\".\"location_geo\""), "{default_sql}");
    }

    /// Every branch leaves exactly one placeholder for the reference geometry
    /// the caller binds, so no branch can bind the wrong number of values.
    #[test]
    fn every_branch_carries_exactly_one_reference_placeholder() {
        let cases = [
            geo_order_distance(&attribute(LOCATION.as_str()), &ResolvedAttributeNames::default(), outer()),
            geo_order_distance(&attribute(CUSTOM_GEOPROPERTY), &resolved_custom(), outer()),
            geo_order_distance(&OrderTarget::Id, &ResolvedAttributeNames::default(), outer()),
        ];
        for sql in cases {
            pretty_assertions::assert_eq!(sql.matches("$1").count(), 1, "{sql}");
        }
    }

    /// An IRI the broker has never recorded keys no attribute, so an ordering on
    /// it measures from nothing rather than failing.
    #[test]
    fn an_unknown_geoproperty_carries_no_geometry() {
        let sql = geo_order_distance(&attribute("https://example.com/neverRecorded"), &ResolvedAttributeNames::default(), outer());
        assert!(sql.contains("NULL::geography"), "{sql}");
    }

    /// The dictionary key is an integer, so no IRI reaches the statement text
    /// and no apostrophe in one can terminate a literal.
    #[test]
    fn no_geoproperty_iri_reaches_the_statement_text() {
        let names = ResolvedAttributeNames::from_pairs([(name("https://example.com/o'brien"), AttributeNameId::from(9_i32))]);
        let sql = geo_order_distance(&attribute("https://example.com/o'brien"), &names, outer());
        assert!(!sql.contains("brien"), "{sql}");
        pretty_assertions::assert_eq!(sql.matches('\'').count() % 2, 0, "unbalanced SQL literal: {sql}");
    }

    /// Clause 4.23 admits `id`, `type` and `scope` as ordering terms; none names
    /// a GeoProperty, so a distance ordering on one measures from nothing.
    #[test]
    fn a_keyword_order_target_carries_no_geometry() {
        for target in [OrderTarget::Id, OrderTarget::Type, OrderTarget::Scope] {
            let sql = geo_order_distance(&target, &ResolvedAttributeNames::default(), outer());
            assert!(sql.contains("NULL::geography"), "{target:?}: {sql}");
        }
    }
}
