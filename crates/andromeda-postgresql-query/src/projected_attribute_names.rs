//! Which attribute names a compiled query has to resolve against the
//! `attribute_names` dictionary before its SQL can be rendered.
//!
//! Five parts of a query reach an attribute by dictionary key, and so have to
//! be collected here: the member existence terms, `attrs` (clause 5.5.9), and
//! the names a `pick` / `omit` projection keeps an entity alive by (clause
//! 4.21), a `q` filter's predicate targets and the sub-attributes on their
//! paths, the `GeoProperty` a geo-query names (clause 4.10), and the attribute
//! and sub-attributes an ordering reads (clause 4.23). Members of a compound
//! value are navigated inside a `value_object` and have no dictionary row to
//! look up.

use crate::q_predicate::{attribute_names::referenced_attribute_names, sub_attribute::member_field::MemberField};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    filter::expression::Expr,
    geo::query::GeoQuery,
    ordering::term::{CompiledOrderTerm, OrderTarget},
};

/// Collect the names a query reads by dictionary key.
///
/// Each of the four axes is optional and contributes independently: a temporal
/// query reads its member names off the dictionary relation rather than by key
/// and orders by time rather than by distance and so passes neither, while a
/// query with no filter and no geometry resolves nothing at all. A member name
/// that does not satisfy the name grammar names no attribute and is skipped;
/// the term it came from then resolves to nothing and matches nothing.
#[must_use]
pub fn projected_attribute_names<'a>(
    members: impl IntoIterator<Item = &'a str>,
    q_expr: Option<&Expr>,
    geo: Option<&GeoQuery>,
    order_terms: &[CompiledOrderTerm],
) -> Vec<AttributeName> {
    let mut names: Vec<AttributeName> = members.into_iter().filter_map(|member| member.parse().ok()).collect();
    if let Some(q_expr) = q_expr {
        referenced_attribute_names(q_expr, &mut names);
    }
    if let Some(geo) = geo {
        // clone: the caller owns the resolved list, which outlives the query
        names.push(geo.geoproperty.clone());
    }
    // Every ordering term, a distance term included, ranks by the clause
    // 4.23.2 value keys read from the attribute's rows, so each resolves its
    // attribute and the sub-attributes on its path. A distance term on a
    // `GeoProperty` other than the default `location` also measures to that
    // attribute's rows by the same key.
    for term in order_terms {
        match &term.target {
            OrderTarget::Attribute(path) => {
                // clone: the caller owns the resolved list, which outlives the query
                names.push(path.attr.clone());
                names.extend(path.sub_attrs.iter().filter(|name| MemberField::of(name).is_none()).cloned());
            }
            // None of the clause 4.23 keywords names an attribute, so none of
            // them has a dictionary row.
            OrderTarget::Id | OrderTarget::Type | OrderTarget::Scope | OrderTarget::CreatedAt | OrderTarget::ModifiedAt => {}
        }
    }
    names
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
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0] })).expect("point parses"),
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
            collected(&projected_attribute_names(attrs.iter().map(IriBuf::as_str), None, None, &[])),
            vec!["https://example.com/speed", "https://example.com/name"]
        );
    }

    /// Clause 4.10: the geo-query's target `GeoProperty` is read by dictionary
    /// key, so it has to be resolved even when the query carries no `q` filter.
    #[test]
    fn a_geo_query_contributes_its_target_geoproperty() {
        let geo = geo_query("https://example.com/observationPoint");
        assert_eq!(
            collected(&projected_attribute_names([], None, Some(&geo), &[])),
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
            collected(&projected_attribute_names([], None, Some(&geo), &[])),
            vec![LOCATION.as_str().to_owned()]
        );
    }

    /// The clause 4.7 `GeoProperties` are stored in the projection like any other
    /// named one, so unlike `location` they do have to be resolved.
    #[test]
    fn a_geo_query_on_a_spec_defined_geoproperty_still_resolves_it() {
        for iri in ["https://uri.etsi.org/ngsi-ld/observationSpace", "https://uri.etsi.org/ngsi-ld/operationSpace"] {
            let geo = geo_query(iri);
            assert_eq!(collected(&projected_attribute_names([], None, Some(&geo), &[])), vec![iri.to_string()]);
        }
    }

    /// Clause 4.23: a distance ordering reads the same projection the filter
    /// does, so its target needs resolving too.
    #[test]
    fn a_distance_ordering_contributes_its_target_geoproperty() {
        let terms = [order_term(ordered_attribute("https://example.com/observationPoint"), OrderDirection::DistAsc)];
        assert_eq!(
            collected(&projected_attribute_names([], None, None, &terms)),
            vec!["https://example.com/observationPoint"]
        );
    }

    /// Clause 4.23.2: an Entity whose `location` is no `GeoProperty` ranks by
    /// its value after the measured ones, and that value is read from its rows
    /// by key, so a distance ordering on the default `GeoProperty` resolves it.
    #[test]
    fn a_distance_ordering_on_the_default_geoproperty_resolves_it_for_its_value_keys() {
        let terms = [order_term(ordered_attribute(LOCATION.as_str()), OrderDirection::DistAsc)];
        assert_eq!(projected_attribute_names([], None, None, &terms), [name(LOCATION.as_str())]);
    }

    /// A distance ordering on a path into a sub-attribute measures nothing and
    /// ranks by the value at the end of the path, read by key.
    #[test]
    fn a_distance_ordering_on_a_sub_attribute_path_resolves_its_names() {
        let mut path = AttrPath::simple(name("https://example.com/area"));
        path.sub_attrs.push(name("https://example.com/accuracy"));
        let terms = [order_term(OrderTarget::Attribute(path), OrderDirection::DistAsc)];
        assert_eq!(
            collected(&projected_attribute_names([], None, None, &terms)),
            vec!["https://example.com/area", "https://example.com/accuracy"]
        );
    }

    /// A value ordering reads the attribute's rows by dictionary key.
    #[test]
    fn a_value_ordering_contributes_its_attribute() {
        let terms = [order_term(ordered_attribute("https://example.com/temperature"), OrderDirection::Asc)];
        assert_eq!(projected_attribute_names([], None, None, &terms), [name("https://example.com/temperature")]);
    }

    /// Clause 4.23's keywords name no attribute, whichever direction they sort.
    #[test]
    fn a_keyword_ordering_contributes_no_name() {
        let terms = [
            order_term(OrderTarget::Id, OrderDirection::DistAsc),
            order_term(OrderTarget::Type, OrderDirection::DistDesc),
            order_term(OrderTarget::Scope, OrderDirection::DistAsc),
        ];
        assert_eq!(projected_attribute_names([], None, None, &terms), [] as [AttributeName; 0]);
    }

    /// The axes accumulate rather than displacing one another.
    #[test]
    fn a_filter_a_geo_query_and_an_ordering_each_contribute_their_own_names() {
        let expr = parser::parse("\"https://example.com/speed\">1").expect("parse q");
        let geo = geo_query("https://example.com/observationPoint");
        let terms = [order_term(ordered_attribute("https://example.com/operationSpace"), OrderDirection::DistAsc)];
        assert_eq!(
            collected(&projected_attribute_names([], Some(&expr), Some(&geo), &terms)),
            vec![
                "https://example.com/speed",
                "https://example.com/observationPoint",
                "https://example.com/operationSpace"
            ]
        );
    }

    #[test]
    fn a_query_with_neither_filter_nor_geometry_resolves_nothing() {
        assert_eq!(projected_attribute_names([], None, None, &[]), [] as [AttributeName; 0]);
    }
}
