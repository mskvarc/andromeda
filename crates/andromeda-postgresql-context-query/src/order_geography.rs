//! The geography expression a distance ordering measures to (clause 4.23).

use andromeda_ngsi_ld_query::ordering::term::OrderTarget;
use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
use andromeda_postgresql_query::{attribute_name_id::AttributeNameId, q_predicate::attribute_names::ResolvedAttributeNames};

/// SQL expression measuring an entity's distance to the reference geometry a
/// distance ordering sorts by (clause 4.23).
///
/// The returned text carries a single `$1` placeholder, which the caller binds
/// the reference geometry to as EWKB.
///
/// The default GeoProperty has a dedicated geography column; every other one is
/// read from its `attribute_current` instance rows, the same projection the geo
/// filter itself reads, so an ordering and a filter can never disagree about
/// where a GeoProperty's geometry lives. Where a GeoProperty has several
/// `datasetId` instances the ordering takes the smallest of their distances,
/// which is the reading that agrees with clause 4.10 counting an entity as
/// matching when any one instance does.
///
/// Clause 4.23's `id` and `type` keywords carry no geometry at all, and neither
/// does a GeoProperty the broker has never recorded, so a distance ordering on
/// either measures from nothing and yields NULL, which sorts every row equally
/// and leaves the `id` tiebreaker to decide.
pub fn geo_order_distance(target: &OrderTarget, attribute_names: &ResolvedAttributeNames) -> String {
    let subject = order_subject(target, attribute_names);
    match subject {
        OrderSubject::None => "ST_Distance(NULL::geography, ST_MakeValid(ST_GeomFromEWKB($1))::geography)".to_string(),
        OrderSubject::LocationColumn => "ST_Distance(\"location_geo\", ST_MakeValid(ST_GeomFromEWKB($1))::geography)".to_string(),
        // The dictionary key is a broker-minted SMALLINT rather than request
        // text, so interpolating it cannot carry an injection.
        OrderSubject::Instances(attribute_id) => format!(
            "(SELECT MIN(ST_Distance(\"{ORDER_ALIAS}\".\"value_geo\"::geography, ST_MakeValid(ST_GeomFromEWKB($1))::geography)) \
             FROM \"attribute_current\" AS \"{ORDER_ALIAS}\" \
             WHERE \"{ORDER_ALIAS}\".\"entity_id\" = \"id\" \
               AND \"{ORDER_ALIAS}\".\"attribute_name_id\" = {} \
               AND \"{ORDER_ALIAS}\".\"value_geo\" IS NOT NULL)",
            attribute_id.get()
        ),
    }
}

/// Alias the correlated ordering subquery reads `attribute_current` under.
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
    let OrderTarget::Attribute(geoproperty_iri) = target else {
        return OrderSubject::None;
    };
    if geoproperty_iri.as_str() == LOCATION.as_str() {
        return OrderSubject::LocationColumn;
    }
    match attribute_names.get(geoproperty_iri.as_str()) {
        Some(attribute_id) => OrderSubject::Instances(attribute_id),
        None => OrderSubject::None,
    }
}

#[cfg(test)]
mod tests {
    use crate::order_geography::geo_order_distance;
    use andromeda_ngsi_ld_query::ordering::term::OrderTarget;
    use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
    use andromeda_postgresql_query::{attribute_name_id::AttributeNameId, q_predicate::attribute_names::ResolvedAttributeNames};
    use iri_rs::IriBuf;

    const CUSTOM_GEOPROPERTY: &str = "https://example.com/customGeo";
    const CUSTOM_GEOPROPERTY_ID: i16 = 42;

    fn attribute(iri: &str) -> OrderTarget {
        OrderTarget::Attribute(IriBuf::new(iri.to_string()).expect("valid IRI"))
    }

    fn resolved_custom() -> ResolvedAttributeNames {
        ResolvedAttributeNames::from_pairs([(
            CUSTOM_GEOPROPERTY.parse().expect("valid attribute name"),
            AttributeNameId::from(CUSTOM_GEOPROPERTY_ID),
        )])
    }

    #[test]
    fn the_default_geoproperty_measures_from_its_dedicated_geography_column() {
        let sql = geo_order_distance(&attribute(LOCATION.as_str()), &ResolvedAttributeNames::default());
        assert!(sql.contains("ST_Distance(\"location_geo\""), "{sql}");
        assert!(!sql.contains("attribute_current"), "the default GeoProperty needs no instance lookup: {sql}");
    }

    /// Clause 4.23 read together with clause 4.10: an ordering reads the same
    /// projection the filter does, so a named GeoProperty measures from its
    /// instance rows and takes the smallest distance among them.
    #[test]
    fn a_custom_geoproperty_measures_from_its_nearest_attribute_instance() {
        let sql = geo_order_distance(&attribute(CUSTOM_GEOPROPERTY), &resolved_custom());
        assert!(sql.contains("FROM \"attribute_current\""), "{sql}");
        assert!(sql.contains("MIN(ST_Distance("), "the nearest instance decides: {sql}");
        assert!(sql.contains(&format!("\"attribute_name_id\" = {CUSTOM_GEOPROPERTY_ID}")), "{sql}");
        assert!(sql.contains("\"geo_order\".\"entity_id\" = \"id\""), "correlated to the ordered row: {sql}");
        assert!(!sql.contains("ST_GeomFromGeoJSON"), "the geometry is stored decoded: {sql}");
    }

    /// Every branch leaves exactly one placeholder for the reference geometry
    /// the caller binds, so no branch can bind the wrong number of values.
    #[test]
    fn every_branch_carries_exactly_one_reference_placeholder() {
        let cases = [
            geo_order_distance(&attribute(LOCATION.as_str()), &ResolvedAttributeNames::default()),
            geo_order_distance(&attribute(CUSTOM_GEOPROPERTY), &resolved_custom()),
            geo_order_distance(&OrderTarget::Id, &ResolvedAttributeNames::default()),
        ];
        for sql in cases {
            pretty_assertions::assert_eq!(sql.matches("$1").count(), 1, "{sql}");
        }
    }

    /// An IRI the broker has never recorded keys no attribute, so an ordering on
    /// it measures from nothing rather than failing.
    #[test]
    fn an_unknown_geoproperty_carries_no_geometry() {
        let sql = geo_order_distance(&attribute("https://example.com/neverRecorded"), &ResolvedAttributeNames::default());
        assert!(sql.contains("NULL::geography"), "{sql}");
    }

    /// The dictionary key is an integer, so no IRI reaches the statement text
    /// and no apostrophe in one can terminate a literal.
    #[test]
    fn no_geoproperty_iri_reaches_the_statement_text() {
        let names = ResolvedAttributeNames::from_pairs([(
            "https://example.com/o'brien".parse().expect("valid attribute name"),
            AttributeNameId::from(9_i16),
        )]);
        let sql = geo_order_distance(&attribute("https://example.com/o'brien"), &names);
        assert!(!sql.contains("brien"), "{sql}");
        pretty_assertions::assert_eq!(sql.matches('\'').count() % 2, 0, "unbalanced SQL literal: {sql}");
    }

    /// Clause 4.23 admits `id`, `type` and `scope` as ordering terms; none names
    /// a GeoProperty, so a distance ordering on one measures from nothing.
    #[test]
    fn a_keyword_order_target_carries_no_geometry() {
        for target in [OrderTarget::Id, OrderTarget::Type, OrderTarget::Scope] {
            let sql = geo_order_distance(&target, &ResolvedAttributeNames::default());
            assert!(sql.contains("NULL::geography"), "{target:?}: {sql}");
        }
    }
}
