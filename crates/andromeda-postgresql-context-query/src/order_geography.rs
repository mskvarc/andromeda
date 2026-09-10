//! The geography expression a distance ordering measures to (clause 4.23).

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
use andromeda_postgresql_query::{
    attribute_instance_correlation::{geometry_instances_of, instance_geometry_column},
    attribute_name_id::AttributeNameId,
    dictionary::resolved_names::ResolvedNames,
    entity_row_refs::EntityRowRefs,
};
use andromeda_postgresql_schema::current::EntitySearch;
use andromeda_postgresql_seaquery::{postgis::validated_geom_from_ewkb, spherical_distance::st_distance_sphere};
use sea_query::{Expr as SeaExpr, Func, QueryStatementBuilder, SimpleExpr};

/// The expression measuring an entity's distance to the reference geometry a
/// distance ordering sorts by (clause 4.23), with the reference geometry bound
/// as EWKB.
///
/// Clause 4.23.1 defines sort by distance as "calculating spherical distance
/// using the Haversine formula", so the measurement is the great-circle one
/// over a sphere, not the spheroidal distance `ST_Distance` makes between two
/// `geography` operands — the two disagree by about a quarter of a percent, and
/// so can rank two entities differently.
///
/// The default `GeoProperty` has a dedicated geometry column; every other one is
/// read from its `current_attribute_values` instance rows, the same projection
/// the geo filter itself reads, so an ordering and a filter can never disagree
/// about where a `GeoProperty`'s geometry lives. Where a `GeoProperty` has several
/// `datasetId` instances the ordering takes the smallest of their distances,
/// which is the reading that agrees with clause 4.10 counting an entity as
/// matching when any one instance does.
///
/// `outer` names the entity rows the ordering sorts, because both renderings
/// are read against them: the dedicated column belongs to the search row, and
/// the instance subquery correlates back to the identity row's surrogate key.
///
/// An attribute IRI the broker has never recorded is carried by no entity, so
/// there is no geometry anywhere to measure to and no distance expression at
/// all — clause 4.23.2 then ranks every entity as one whose "attribute is not a
/// `GeoProperty`" and orders them by value instead.
#[must_use]
pub fn geo_order_distance(
    geoproperty: &AttributeName,
    attribute_names: &ResolvedNames<AttributeName>,
    outer: &EntityRowRefs,
    reference: &[u8],
) -> Option<SimpleExpr> {
    // Built fresh per branch so the EWKB it binds is not shared as an owned value.
    let reference_geometry = || validated_geom_from_ewkb(SeaExpr::val(reference.to_vec()));
    match order_subject(geoproperty, attribute_names) {
        OrderSubject::None => None,
        OrderSubject::LocationColumn => Some(st_distance_sphere(outer.search_column(EntitySearch::Location), reference_geometry())),
        OrderSubject::Instances(attribute_id) => {
            let mut scan = geometry_instances_of(outer, ORDER_ALIAS, attribute_id);
            scan.expr(Func::min(st_distance_sphere(instance_geometry_column(ORDER_ALIAS), reference_geometry())));
            Some(SimpleExpr::SubQuery(None, Box::new(scan.into_sub_query_statement())))
        }
    }
}

/// Alias the correlated ordering subquery reads `current_attribute_values`
/// under.
///
/// Deliberately not the alias the geo predicate uses: a query that both filters
/// and orders on `GeoProperties` nests the two subqueries in one statement.
const ORDER_ALIAS: &str = "geo_order";

/// Where a distance ordering reads the entity's geometry from.
enum OrderSubject {
    /// No entity carries a geometry under this name.
    None,
    /// The default `GeoProperty`'s dedicated geometry column.
    LocationColumn,
    /// The instance rows of one attribute, by dictionary key.
    Instances(AttributeNameId),
}

/// Classify an ordering term by where its geometry is stored.
fn order_subject(geoproperty: &AttributeName, attribute_names: &ResolvedNames<AttributeName>) -> OrderSubject {
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
    use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
    use andromeda_postgresql_query::{
        attribute_name_id::AttributeNameId,
        dictionary::resolved_names::ResolvedNames,
        entity_row_refs::{DocumentsRef, EntityRowRefs},
    };
    use sea_query::{Alias, PostgresQueryBuilder, Query, SimpleExpr};

    const CUSTOM_GEOPROPERTY: &str = "https://example.com/customGeo";
    const CUSTOM_GEOPROPERTY_ID: i32 = 42;

    /// A stand-in reference geometry the distance expression binds.
    const REFERENCE: &[u8] = &[1, 2, 3, 4];

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    fn resolved_custom() -> ResolvedNames<AttributeName> {
        ResolvedNames::from_pairs([(name(CUSTOM_GEOPROPERTY), AttributeNameId::from(CUSTOM_GEOPROPERTY_ID))])
    }

    /// The entity statement selects the entity relations unaliased.
    fn outer() -> EntityRowRefs {
        EntityRowRefs::unaliased()
    }

    /// The rows as the temporal path aliases them.
    fn aliased() -> EntityRowRefs {
        EntityRowRefs::aliased(Alias::new("e"), Alias::new("s"), DocumentsRef::ByKeyLookup)
    }

    /// Render a distance expression to its SQL with values inlined.
    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    /// Render a distance expression to its SQL with `$n` placeholders.
    fn render_parameterised(expr: SimpleExpr) -> String {
        let (sql, _) = Query::select().expr(expr).to_owned().build(PostgresQueryBuilder);
        sql
    }

    /// Clause 4.23.1 defines sort by distance as the Haversine spherical
    /// distance, which is not the spheroidal measurement two `geography`
    /// operands make.
    #[test]
    fn the_distance_is_measured_on_a_sphere_and_not_on_the_spheroid() {
        let sql = render(
            geo_order_distance(&name(LOCATION.as_str()), &ResolvedNames::default(), &outer(), REFERENCE).expect("the default GeoProperty is measurable"),
        );
        assert!(sql.contains("ST_DistanceSphere("), "{sql}");
        assert!(!sql.contains("::geography"), "the spheroidal measurement must not be used: {sql}");
    }

    #[test]
    fn the_default_geoproperty_measures_from_its_dedicated_geometry_column() {
        let sql = render(
            geo_order_distance(&name(LOCATION.as_str()), &ResolvedNames::default(), &outer(), REFERENCE).expect("the default GeoProperty is measurable"),
        );
        assert!(sql.contains("ST_DistanceSphere(\"entity_search\".\"location\""), "{sql}");
        assert!(
            !sql.contains("current_attribute_values"),
            "the default GeoProperty needs no instance lookup: {sql}"
        );
    }

    /// Clause 4.23 read together with clause 4.10: an ordering reads the same
    /// projection the filter does, so a named `GeoProperty` measures from its
    /// instance rows and takes the smallest distance among them.
    #[test]
    fn a_custom_geoproperty_measures_from_its_nearest_attribute_instance() {
        let sql = render(geo_order_distance(&name(CUSTOM_GEOPROPERTY), &resolved_custom(), &outer(), REFERENCE).expect("a recorded GeoProperty is measurable"));
        assert!(sql.contains("FROM \"current_attribute_values\""), "{sql}");
        assert!(sql.contains("MIN(ST_DistanceSphere("), "the nearest instance decides: {sql}");
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
        let sql =
            render(geo_order_distance(&name(CUSTOM_GEOPROPERTY), &resolved_custom(), &aliased(), REFERENCE).expect("a recorded GeoProperty is measurable"));
        assert!(sql.contains("\"geo_order\".\"entity_key\" = \"e\".\"entity_key\""), "{sql}");
        let default_sql = render(
            geo_order_distance(&name(LOCATION.as_str()), &ResolvedNames::default(), &aliased(), REFERENCE).expect("the default GeoProperty is measurable"),
        );
        assert!(default_sql.contains("ST_DistanceSphere(\"s\".\"location\""), "{default_sql}");
    }

    /// Every branch that measures anything binds the reference geometry as its
    /// first parameter, so no branch can bind the wrong number of values.
    #[test]
    fn every_measuring_branch_binds_the_reference_geometry_first() {
        let cases = [
            geo_order_distance(&name(LOCATION.as_str()), &ResolvedNames::default(), &outer(), REFERENCE),
            geo_order_distance(&name(CUSTOM_GEOPROPERTY), &resolved_custom(), &outer(), REFERENCE),
        ];
        for expr in cases {
            let sql = render_parameterised(expr.expect("a measurable GeoProperty"));
            assert_eq!(sql.matches("$1").count(), 1, "{sql}");
        }
    }

    /// The dictionary key is an integer, so no IRI reaches the statement text
    /// and no apostrophe in one can terminate a literal.
    #[test]
    fn no_geoproperty_iri_reaches_the_statement_text() {
        let names = ResolvedNames::from_pairs([(name("https://example.com/o'brien"), AttributeNameId::from(9_i32))]);
        let sql = render(geo_order_distance(&name("https://example.com/o'brien"), &names, &outer(), REFERENCE).expect("a recorded GeoProperty is measurable"));
        assert!(!sql.contains("brien"), "{sql}");
        assert_eq!(sql.matches('\'').count() % 2, 0, "unbalanced SQL literal: {sql}");
    }

    /// An IRI the broker has never recorded keys no attribute, so there is no
    /// geometry to measure to and the ordering falls through to clause 4.23.2's
    /// comparison by value rather than measuring to a silent `NULL`.
    #[test]
    fn an_unknown_geoproperty_yields_no_distance_expression_at_all() {
        assert!(geo_order_distance(&name("https://example.com/neverRecorded"), &ResolvedNames::default(), &outer(), REFERENCE).is_none());
    }
}
