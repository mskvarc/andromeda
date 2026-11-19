//! The geography expression a distance ordering measures to (clause 4.23).

use crate::{
    attribute_instance_correlation::{geometry_instances_of, instance_geometry_column},
    entity_row_refs::EntityRowRefs,
    standing_instant::instant_of,
};
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName};
use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
use andromeda_postgresql_schema::current::EntitySearch;
use andromeda_postgresql_seaquery::{postgis::validated_geom_from_ewkb, spherical_distance::st_distance_sphere};
use andromeda_postgresql_stored_form::expiry::unexpired;
use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
use sea_query::{CaseStatement, Expr as SeaExpr, Func, Keyword, QueryStatementBuilder, SimpleExpr};

/// The expression measuring an entity's distance to the reference geometry a
/// distance ordering sorts by (clause 4.23), with the reference geometry bound
/// as EWKB.
///
/// Clause 4.23.1 defines sort by distance as "calculating spherical distance
/// using the Haversine formula", so the measurement is the great-circle one
/// over a sphere, not the spheroidal distance `ST_Distance` makes between two
/// `geography` operands; the two disagree by about a quarter of a percent, and
/// so can rank two entities differently.
///
/// The default `GeoProperty` has a dedicated geometry column; every other one is
/// read from its `current_attributes` instance rows, the same rows the geo
/// filter itself reads, so an ordering and a filter can never disagree
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
/// all; clause 4.23.2 then ranks every entity as one whose "attribute is not a
/// `GeoProperty`" and orders them by value instead.
#[must_use]
pub fn geo_order_distance(
    geoproperty: &AttributeIri,
    attribute_names: &ResolvedNames<AttributeName>,
    outer: &EntityRowRefs,
    reference: &[u8],
) -> Option<SimpleExpr> {
    // Built fresh per branch so the EWKB it binds is not shared as an owned value.
    let reference_geometry = || validated_geom_from_ewkb(SeaExpr::val(reference.to_vec()));
    match order_subject(geoproperty, attribute_names) {
        OrderSubject::None => None,
        // Clause 4.22: past its own `expiresAt` the hoisted instance holds no
        // geometry to measure to, exactly as an expired instance row does not.
        OrderSubject::LocationColumn => Some(
            CaseStatement::new()
                .case(
                    unexpired(outer.search_column(EntitySearch::LocationExpiresAt), instant_of(outer.standing())),
                    st_distance_sphere(outer.search_column(EntitySearch::Location), reference_geometry()),
                )
                .finally(Keyword::Null)
                .into(),
        ),
        OrderSubject::Instances(attribute_name_key) => {
            let mut scan = geometry_instances_of(outer, ORDER_ALIAS, attribute_name_key);
            scan.expr(Func::min(st_distance_sphere(instance_geometry_column(ORDER_ALIAS), reference_geometry())));
            Some(SimpleExpr::SubQuery(None, Box::new(scan.into_sub_query_statement())))
        }
    }
}

/// Alias the correlated ordering subquery reads `current_attributes`
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
    Instances(AttributeNameKey),
}

/// Classify an ordering term by where its geometry is stored.
fn order_subject(geoproperty: &AttributeIri, attribute_names: &ResolvedNames<AttributeName>) -> OrderSubject {
    if geoproperty.as_str() == LOCATION.as_str() {
        return OrderSubject::LocationColumn;
    }
    match attribute_names.get_str(geoproperty.as_str()) {
        Some(attribute_name_key) => OrderSubject::Instances(attribute_name_key),
        None => OrderSubject::None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_row_refs::EntityRowRefs, ordering::order_geography::geo_order_distance};
    use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName};
    use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use sea_query::{Alias, Query, SelectStatement, SimpleExpr};

    const CUSTOM_GEOPROPERTY: &str = "https://example.com/customGeo";
    const CUSTOM_GEOPROPERTY_ID: i32 = 42;

    /// A stand-in reference geometry the distance expression binds.
    const REFERENCE: &[u8] = &[1, 2, 3, 4];

    fn name(value: &str) -> AttributeIri {
        value.parse().expect("an absolute IRI")
    }

    fn resolved_custom() -> ResolvedNames<AttributeName> {
        ResolvedNames::from_pairs([(
            CUSTOM_GEOPROPERTY.parse().expect("valid attribute name"),
            AttributeNameKey::from(CUSTOM_GEOPROPERTY_ID),
        )])
    }

    /// The entity statement selects the entity relations unaliased.
    fn outer() -> EntityRowRefs {
        EntityRowRefs::unaliased(None)
    }

    /// The rows as the temporal path aliases them.
    fn aliased() -> EntityRowRefs {
        EntityRowRefs::aliased(None, Alias::new("e"), Alias::new("s"))
    }

    /// Wrap a distance expression in the statement that selects it.
    fn render(expr: SimpleExpr) -> SelectStatement {
        Query::select().expr(expr).to_owned()
    }

    /// Clause 4.23.1 defines sort by distance as the Haversine spherical
    /// distance, which is not the spheroidal measurement two `geography`
    /// operands make.
    #[test]
    fn the_distance_is_measured_on_a_sphere_and_not_on_the_spheroid() {
        let select = render(
            geo_order_distance(&name(LOCATION.as_str()), &ResolvedNames::default(), &outer(), REFERENCE).expect("the default GeoProperty is measurable"),
        );
        assert_statement_snapshot!("order-geography-the-distance-is-measured-on-a-sphere-and-not-on-the-spheroid", select);
    }

    #[test]
    fn the_default_geoproperty_measures_from_its_dedicated_geometry_column() {
        let select = render(
            geo_order_distance(&name(LOCATION.as_str()), &ResolvedNames::default(), &outer(), REFERENCE).expect("the default GeoProperty is measurable"),
        );
        assert_statement_snapshot!("order-geography-the-default-geoproperty-measures-from-its-dedicated-geometry-column", select);
    }

    /// Clause 4.23 read together with clause 4.10: an ordering reads the same
    /// rows the filter does, so a named `GeoProperty` measures from its
    /// instance rows and takes the smallest distance among them.
    #[test]
    fn a_custom_geoproperty_measures_from_its_nearest_attribute_instance() {
        let select =
            render(geo_order_distance(&name(CUSTOM_GEOPROPERTY), &resolved_custom(), &outer(), REFERENCE).expect("a recorded GeoProperty is measurable"));
        assert_statement_snapshot!("order-geography-a-custom-geoproperty-measures-from-its-nearest-attribute-instance", select);
    }

    /// The correlation binds the outer relation it was handed rather than
    /// whichever `entity_key` column the enclosing statement happens to expose.
    #[test]
    fn the_ordering_subquery_correlates_through_the_outer_row_it_is_given() {
        let custom =
            render(geo_order_distance(&name(CUSTOM_GEOPROPERTY), &resolved_custom(), &aliased(), REFERENCE).expect("a recorded GeoProperty is measurable"));
        assert_statement_snapshot!("order-geography-the-ordering-subquery-correlates-through-the-outer-row-it-is-given-1", custom);
        let default = render(
            geo_order_distance(&name(LOCATION.as_str()), &ResolvedNames::default(), &aliased(), REFERENCE).expect("the default GeoProperty is measurable"),
        );
        assert_statement_snapshot!("order-geography-the-ordering-subquery-correlates-through-the-outer-row-it-is-given-2", default);
    }

    /// Every branch that measures anything binds the reference geometry as its
    /// first parameter, so no branch can bind the wrong number of values.
    #[test]
    fn every_measuring_branch_binds_the_reference_geometry_first() {
        let cases = [
            geo_order_distance(&name(LOCATION.as_str()), &ResolvedNames::default(), &outer(), REFERENCE),
            geo_order_distance(&name(CUSTOM_GEOPROPERTY), &resolved_custom(), &outer(), REFERENCE),
        ];
        for (index, expr) in cases.into_iter().enumerate() {
            let select = render(expr.expect("a measurable GeoProperty"));
            assert_statement_snapshot!(
                format!("order-geography-every-measuring-branch-binds-the-reference-geometry-first-{}", index + 1),
                select
            );
        }
    }

    /// The dictionary key is an integer, so no IRI reaches the statement text
    /// and no apostrophe in one can terminate a literal.
    #[test]
    fn no_geoproperty_iri_reaches_the_statement_text() {
        let names = ResolvedNames::from_pairs([(
            "https://example.com/o'brien".parse().expect("valid attribute name"),
            AttributeNameKey::from(9_i32),
        )]);
        let select =
            render(geo_order_distance(&name("https://example.com/o'brien"), &names, &outer(), REFERENCE).expect("a recorded GeoProperty is measurable"));
        assert_statement_snapshot!("order-geography-no-geoproperty-iri-reaches-the-statement-text", select);
    }

    /// An IRI the broker has never recorded keys no attribute, so there is no
    /// geometry to measure to and the ordering falls through to clause 4.23.2's
    /// comparison by value rather than measuring to a silent `NULL`.
    #[test]
    fn an_unknown_geoproperty_yields_no_distance_expression_at_all() {
        assert!(geo_order_distance(&name("https://example.com/neverRecorded"), &ResolvedNames::default(), &outer(), REFERENCE).is_none());
    }
}
