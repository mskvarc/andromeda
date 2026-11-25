//! The Attribute instances `q` and the geo-query keep in a temporal
//! representation (clause 5.7.4.4).
//!
//! The last step of clause 5.7.4.4 includes only the instances "which meet the
//! temporal, query and geoquery restrictions". An instance of an Attribute
//! that `q` or the geo-query names meets them when both still hold with that
//! Attribute reduced to that instance; an instance of any other Attribute
//! meets them as its Entity does. A Scope selects Entities only (clauses 4.19
//! and 5.7.4.4 step S4), so it narrows nothing.
//!
//! A store renders the rule over the rows it reads. An Entity gathered from
//! several parts is narrowed in memory instead, over the truths of the leaves
//! of the filters, since no statement over one part reads the others. Both
//! read the Attributes the rule narrows and the formula from here.

use crate::{
    entity_selection::EntitySelection,
    filter::{compiled_filter::CompiledFilter, reading::filter_reading::FilterReading},
    geo::compiled_geo_query::CompiledGeoQuery,
    split_filter::{FilterLeaf, LeafTruths, SplitFilter},
};
use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;

/// The restriction the `q` filter and the geo-query of a query place on the
/// instances of a temporal representation.
#[derive(Debug, Clone, Copy)]
pub struct InstanceRestriction<'q> {
    /// The filters of the query, read as a formula over their leaves.
    filter: SplitFilter<'q>,
}

impl<'q> InstanceRestriction<'q> {
    /// The restriction the filters of `selection` place on the instances.
    #[must_use]
    pub const fn of(selection: &'q EntitySelection) -> InstanceRestriction<'q> {
        InstanceRestriction {
            filter: SplitFilter::of(selection),
        }
    }

    /// The reading of the `q` filter an instance of a narrowed Attribute is
    /// checked against, when the query carries one.
    #[must_use]
    pub fn q(&self) -> Option<&'q FilterReading> {
        self.filter.filters().q.map(CompiledFilter::reading)
    }

    /// The geo-query an instance of a narrowed Attribute is checked against,
    /// when the query carries one.
    #[must_use]
    pub const fn geo(&self) -> Option<&'q CompiledGeoQuery> {
        self.filter.filters().geo
    }

    /// The Attributes whose instances the restriction narrows, each once, in
    /// the order of the leaves that read them: every Query Term's own
    /// Attribute or the Relationship its first hop crosses, then the
    /// geo-query's `GeoProperty`. Empty when the query carries neither `q` nor
    /// a geo-query.
    #[must_use]
    pub fn narrowed_attributes(&self) -> Vec<&'q AttributeIri> {
        let mut attributes: Vec<&'q AttributeIri> = Vec::new();
        for attribute in self.filter.leaves().filter_map(FilterLeaf::entity_attribute) {
            if !attributes.contains(&attribute) {
                attributes.push(attribute);
            }
        }
        attributes
    }

    /// The filters whose leaves the in-memory execution reads truths of, in
    /// the order [`SplitFilter::leaves`] yields them.
    #[must_use]
    pub const fn filter(&self) -> SplitFilter<'q> {
        self.filter
    }

    /// Whether an instance of `attribute` meets the restriction, which it
    /// does when `q` and the geo-query hold with every leaf reading
    /// `attribute` as `instance` says, read over that instance alone, and
    /// every other leaf as `entity` says, read over the whole Entity.
    #[must_use]
    pub fn keeps(&self, attribute: &AttributeIri, entity: &LeafTruths, instance: &LeafTruths) -> bool {
        let reduced = self
            .filter
            .leaves()
            .enumerate()
            .map(|(index, leaf)| {
                if leaf.entity_attribute() == Some(attribute) {
                    instance.get(index)
                } else {
                    entity.get(index)
                }
            })
            .collect();
        self.filter.values_hold(&LeafTruths::from_vec(reduced))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_selection::EntitySelection,
        filter::{Filter, compiled_filter::CompiledFilter, language_matching::LanguageMatching},
        geo::compiled_geo_query::CompiledGeoQuery,
        name_stage::Written,
        namespace_expander::EXAMPLE_NAMESPACE,
        scope::scope_query::ScopeQuery,
        split_filter::LeafTruths,
        temporal::instance_restriction::InstanceRestriction,
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;
    use serde_json::json;

    const SPEED: &str = "https://example.com/speed";
    const FUEL: &str = "https://example.com/fuelLevel";
    const PARKED: &str = "https://example.com/parkedIn";
    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    fn attribute(raw: &str) -> AttributeIri {
        raw.parse().expect("a valid attribute name")
    }

    fn within_berlin() -> CompiledGeoQuery {
        CompiledGeoQuery {
            georel: GeoRel::Within,
            geometry: serde_json::from_value::<Geometry>(json!({
                "type": "Polygon",
                "coordinates": [[[13.0, 52.0], [14.0, 52.0], [14.0, 53.0], [13.0, 53.0], [13.0, 52.0]]]
            }))
            .expect("polygon parses"),
            geoproperty: attribute(LOCATION),
        }
    }

    fn selection(q: Option<&str>, geo: Option<CompiledGeoQuery>, scope: Option<&str>) -> EntitySelection {
        EntitySelection::builder()
            .q(q.map(|raw| {
                CompiledFilter::new(
                    EXAMPLE_NAMESPACE
                        .expanded(raw.parse::<Filter<Written>>().expect("a clause 4.9 filter"))
                        .expect("the names expand"),
                    LanguageMatching::TagEquality,
                )
            }))
            .geo(geo)
            .scope_q(scope.map(|raw| raw.parse::<ScopeQuery>().expect("scopeQ parses")))
            .build()
    }

    /// Every Query Term's Attribute is narrowed once, a hop by the
    /// Relationship it crosses from the Entity, and the geo-query's
    /// `GeoProperty` after them.
    #[test]
    fn the_narrowed_attributes_are_the_ones_the_leaves_read_of_the_entity() {
        let selection = selection(
            Some(&format!("\"{SPEED}\">90|\"{SPEED}\"<10;\"{PARKED}\"{{\"{FUEL}\"}}>0")),
            Some(within_berlin()),
            Some("/Madrid"),
        );
        let restriction = InstanceRestriction::of(&selection);
        let names: Vec<&str> = restriction.narrowed_attributes().into_iter().map(AttributeIri::as_str).collect();
        pretty_assertions::assert_eq!(names, [SPEED, PARKED, LOCATION]);
    }

    /// A Scope query alone narrows nothing.
    #[test]
    fn a_scope_query_alone_narrows_nothing() {
        let selection = selection(None, None, Some("/Madrid"));
        pretty_assertions::assert_eq!(InstanceRestriction::of(&selection).narrowed_attributes(), Vec::<&AttributeIri>::new());
    }

    /// The truths of the leaves of `restriction`, each leaf holding as `held`
    /// says of the Attribute it reads.
    fn truths_by_attribute(restriction: &InstanceRestriction<'_>, held: impl Fn(&str) -> bool) -> LeafTruths {
        LeafTruths::from_vec(
            restriction
                .filter()
                .leaves()
                .map(|leaf| leaf.entity_attribute().is_some_and(|attribute| held(attribute.as_str())))
                .collect(),
        )
    }

    /// `speed>90|fuelLevel<35` keeps an instance of `fuelLevel` when `speed`
    /// held over the Entity, whatever the instance holds.
    #[test]
    fn a_disjunction_keeps_an_instance_through_another_attribute() {
        let selection = selection(Some(&format!("\"{SPEED}\">90|\"{FUEL}\"<35")), None, None);
        let restriction = InstanceRestriction::of(&selection);
        let entity = truths_by_attribute(&restriction, |name| name == SPEED);
        let nothing = truths_by_attribute(&restriction, |_| false);
        let speed = truths_by_attribute(&restriction, |name| name == SPEED);
        assert!(restriction.keeps(&attribute(FUEL), &entity, &nothing));
        assert!(!restriction.keeps(&attribute(SPEED), &entity, &nothing));
        assert!(restriction.keeps(&attribute(SPEED), &entity, &speed));
    }

    /// `speed<50;speed>90` reads both terms over the one instance of `speed`,
    /// so no single instance meets it even when the Entity does.
    #[test]
    fn a_conjunction_over_one_attribute_reads_both_terms_over_the_instance() {
        let selection = selection(Some(&format!("\"{SPEED}\"<50;\"{SPEED}\">90")), None, None);
        let restriction = InstanceRestriction::of(&selection);
        let entity = LeafTruths::from_vec(vec![true, true]);
        assert!(!restriction.keeps(&attribute(SPEED), &entity, &LeafTruths::from_vec(vec![true, false])));
        assert!(!restriction.keeps(&attribute(SPEED), &entity, &LeafTruths::from_vec(vec![false, true])));
    }

    /// The Scope query is not read: an instance is kept on `q` and the
    /// geo-query alone.
    #[test]
    fn the_scope_query_is_left_unread() {
        let selection = selection(Some(&format!("\"{SPEED}\">90")), Some(within_berlin()), Some("/Madrid"));
        let restriction = InstanceRestriction::of(&selection);
        let entity = truths_by_attribute(&restriction, |name| name == LOCATION);
        let speed = truths_by_attribute(&restriction, |name| name == SPEED);
        let nothing = truths_by_attribute(&restriction, |_| false);
        assert!(restriction.keeps(&attribute(SPEED), &entity, &speed));
        assert!(!restriction.keeps(&attribute(LOCATION), &entity, &nothing));
    }
}
