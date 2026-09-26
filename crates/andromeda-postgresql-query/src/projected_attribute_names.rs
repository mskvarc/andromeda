//! Which attribute names a compiled query has to resolve against the
//! `attribute_names` dictionary before its SQL can be rendered.
//!
//! Four parts of a query reach an attribute by dictionary key rather than
//! through the JSONB payload, and so have to be collected here: the `attrs`
//! existence terms (clause 5.5.9), a `q` filter's predicate targets, the
//! `GeoProperty` a geo-query names (clause 4.10), and the `GeoProperty` a distance
//! ordering measures to (clause 4.23). Everything else is navigated inside the
//! payload and has no dictionary row to look up.

use crate::q_predicate::attribute_names::referenced_attribute_names;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    filter::expression::Expr,
    geo::query::GeoQuery,
    ordering::term::{CompiledOrderTerm, OrderTarget},
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
use iri_rs::IriBuf;

/// Collect the names a query reads by dictionary key.
///
/// Each of the four axes is optional and contributes independently: a temporal
/// query has no `attrs` axis and orders by time rather than by distance and so
/// passes neither, while a query with no filter and no geometry resolves
/// nothing at all. An `attrs` IRI that does not satisfy the name grammar
/// names no attribute and is skipped; the term it came from then resolves to
/// nothing and matches nothing.
#[must_use]
pub fn projected_attribute_names(attrs: &[IriBuf], q_expr: Option<&Expr>, geo: Option<&GeoQuery>, order_terms: &[CompiledOrderTerm]) -> Vec<AttributeName> {
    let mut names: Vec<AttributeName> = attrs.iter().filter_map(|attr| attr.as_str().parse().ok()).collect();
    if let Some(q_expr) = q_expr {
        referenced_attribute_names(q_expr, &mut names);
    }
    if let Some(geo) = geo {
        // clone: the caller owns the resolved list, which outlives the query
        names.push(geo.geoproperty.clone());
    }
    for term in order_terms.iter().filter(|term| term.direction.is_distance()) {
        match &term.target {
            // Only the attribute itself can be the `GeoProperty` a distance is
            // measured to. Clause 4.23.1 sorts by distance when "Attribute is a
            // GeoProperty", and a path that descends into a sub-attribute or
            // into a member of a compound value addresses a value inside one,
            // which carries no geometry of its own — so it resolves no
            // dictionary row and falls through to the clause 4.23.2 value keys.
            OrderTarget::Attribute(path) if path.is_simple() => push_distance_target(&path.attr, &mut names),
            // None of the clause 4.23 keywords names an attribute, so none of
            // them has a dictionary row.
            OrderTarget::Attribute(_) | OrderTarget::Id | OrderTarget::Type | OrderTarget::Scope | OrderTarget::CreatedAt | OrderTarget::ModifiedAt => {}
        }
    }
    names
}

/// Add the `GeoProperty` a distance ordering measures to, unless it is the one
/// that needs no dictionary row.
///
/// A geo-query always resolves its target, the default `location` included:
/// clause 5.7.2.4 lets any instance of it match, and every instance but the
/// hoisted default one is a `current_attribute_values` row keyed by the
/// dictionary. A distance ordering on `location` (clause 4.23) measures from
/// the hoisted default instance's dedicated `geography` column alone, so it
/// keys nothing; every other `GeoProperty`, the clause 4.7 `observationSpace`
/// and `operationSpace` included, goes through the projection and is
/// collected.
fn push_distance_target(name: &AttributeName, out: &mut Vec<AttributeName>) {
    if name.as_str() == LOCATION.as_str() {
        return;
    }
    // clone: the caller owns the resolved list, which outlives the query
    out.push(name.clone());
}

#[cfg(test)]
mod tests {
    use crate::projected_attribute_names::projected_attribute_names;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::{
        filter::{parser, path::AttrPath},
        geo::query::GeoQuery,
        ordering::term::{CompiledOrderTerm, OrderDirection, OrderTarget},
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    /// A distance ordering names one attribute, which is the simple path.
    fn ordered_attribute(value: &str) -> OrderTarget {
        OrderTarget::Attribute(AttrPath::simple(name(value)))
    }

    fn geo_query(geoproperty: &str) -> GeoQuery {
        GeoQuery {
            georel: GeoRel::Within,
            geometry: Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0] })).expect("point parses"),
            geoproperty: name(geoproperty),
        }
    }

    fn order_term(target: OrderTarget, direction: OrderDirection) -> CompiledOrderTerm {
        CompiledOrderTerm {
            target,
            direction,
            collation: None,
        }
    }

    fn collected(names: &[AttributeName]) -> Vec<String> {
        names.iter().map(AttributeName::as_str).map(str::to_string).collect()
    }

    /// Clause 5.5.9: every `attrs` term is answered by dictionary key, so each
    /// one is resolved; an IRI that cannot name an attribute is skipped.
    #[test]
    fn attrs_terms_contribute_their_names_and_skip_unparsable_iris() {
        let attrs = [
            IriBuf::new("https://example.com/speed".to_string()).expect("valid IRI"),
            IriBuf::new("https://example.com/name".to_string()).expect("valid IRI"),
        ];
        assert_eq!(
            collected(&projected_attribute_names(&attrs, None, None, &[])),
            vec!["https://example.com/speed", "https://example.com/name"]
        );
    }

    /// Clause 4.10: the geo-query's target `GeoProperty` is read by dictionary
    /// key, so it has to be resolved even when the query carries no `q` filter.
    #[test]
    fn a_geo_query_contributes_its_target_geoproperty() {
        let geo = geo_query("https://example.com/observationPoint");
        assert_eq!(
            collected(&projected_attribute_names(&[], None, Some(&geo), &[])),
            vec!["https://example.com/observationPoint"]
        );
    }

    /// Clause 5.7.2.4 lets any `location` instance match, and every instance
    /// carrying a `datasetId` is a projection row keyed by the dictionary, so a
    /// geo-query on the default `GeoProperty` resolves it too.
    #[test]
    fn a_geo_query_on_the_default_geoproperty_resolves_it_for_its_dataset_instances() {
        let geo = geo_query(LOCATION.as_str());
        assert_eq!(
            collected(&projected_attribute_names(&[], None, Some(&geo), &[])),
            vec![LOCATION.as_str().to_owned()]
        );
    }

    /// The clause 4.7 `GeoProperties` are stored in the projection like any other
    /// named one, so unlike `location` they do have to be resolved.
    #[test]
    fn a_geo_query_on_a_spec_defined_geoproperty_still_resolves_it() {
        for iri in ["https://uri.etsi.org/ngsi-ld/observationSpace", "https://uri.etsi.org/ngsi-ld/operationSpace"] {
            let geo = geo_query(iri);
            assert_eq!(collected(&projected_attribute_names(&[], None, Some(&geo), &[])), vec![iri.to_string()]);
        }
    }

    /// Clause 4.23: a distance ordering reads the same projection the filter
    /// does, so its target needs resolving too.
    #[test]
    fn a_distance_ordering_contributes_its_target_geoproperty() {
        let terms = [order_term(ordered_attribute("https://example.com/observationPoint"), OrderDirection::DistAsc)];
        assert_eq!(
            collected(&projected_attribute_names(&[], None, None, &terms)),
            vec!["https://example.com/observationPoint"]
        );
    }

    /// A distance ordering on the default `GeoProperty` measures from the
    /// dedicated geography column, so it resolves nothing either.
    #[test]
    fn a_distance_ordering_on_the_default_geoproperty_resolves_nothing() {
        let terms = [order_term(ordered_attribute(LOCATION.as_str()), OrderDirection::DistAsc)];
        assert_eq!(projected_attribute_names(&[], None, None, &terms), [] as [AttributeName; 0]);
    }

    /// A value ordering navigates the payload, so it contributes nothing.
    #[test]
    fn a_value_ordering_contributes_no_name() {
        let terms = [order_term(ordered_attribute("https://example.com/temperature"), OrderDirection::Asc)];
        assert_eq!(projected_attribute_names(&[], None, None, &terms), [] as [AttributeName; 0]);
    }

    /// Clause 4.23's keywords name no attribute, whichever direction they sort.
    #[test]
    fn a_keyword_ordering_contributes_no_name() {
        let terms = [
            order_term(OrderTarget::Id, OrderDirection::DistAsc),
            order_term(OrderTarget::Type, OrderDirection::DistDesc),
            order_term(OrderTarget::Scope, OrderDirection::DistAsc),
        ];
        assert_eq!(projected_attribute_names(&[], None, None, &terms), [] as [AttributeName; 0]);
    }

    /// The three axes accumulate rather than displacing one another.
    #[test]
    fn a_filter_a_geo_query_and_an_ordering_each_contribute_their_own_names() {
        let expr = parser::parse("\"https://example.com/speed\">1").expect("parse q");
        let geo = geo_query("https://example.com/observationPoint");
        let terms = [order_term(ordered_attribute("https://example.com/operationSpace"), OrderDirection::DistAsc)];
        assert_eq!(
            collected(&projected_attribute_names(&[], Some(&expr), Some(&geo), &terms)),
            vec![
                "https://example.com/speed",
                "https://example.com/observationPoint",
                "https://example.com/operationSpace"
            ]
        );
    }

    #[test]
    fn a_query_with_neither_filter_nor_geometry_resolves_nothing() {
        assert_eq!(projected_attribute_names(&[], None, None, &[]), [] as [AttributeName; 0]);
    }
}
