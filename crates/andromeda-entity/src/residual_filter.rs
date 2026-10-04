//! In-memory evaluation of residual predicates.
//!
//! The [`ResidualFilter`] type itself lives beside the store port, which owns
//! its construction and structural inspection. This module adds the
//! in-memory evaluator that finishes what a store declined, reading the
//! entity through the query languages' read traits: a stored entity state, or
//! the written Temporal Evolution document a deferred temporal write observed.
//!
//! [`ResidualFilter`]: crate::query_residual::ResidualFilter

use crate::query_residual::ResidualFilter;
use andromeda_ngsi_ld_entity::{
    document::{EntityDocument, slot::Slot},
    entity_state::EntityState,
};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_query::{
    entity_selection::{ConsideredEntities, EntitySelection, EntitySelector},
    filter::{eval::evaluate, expression::Expr},
    geo::query::GeoQuery,
    pattern::Pattern,
    projection::projected_members::{ProjectedMembers, ScopeRetention},
    queried_entity::{attribute_reading::QueriedEntity, linked_entities::NoLinkedEntities, located_entity::LocatedEntity, scoped_entity::ScopedEntity},
    scope::query::ScopeQuery,
    type_selection::grammar::TypeExpr,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use iri_rs::IriBuf;

/// Whether one stored entity satisfies every outstanding predicate.
///
/// Every predicate must hold at once, which is the clause 4.9 conjunction
/// the store would have rendered as `AND`.
#[must_use]
pub fn matches(residual: &ResidualFilter, state: &EntityState) -> bool {
    matches_selected(residual, state)
}

/// Whether one written Temporal Evolution document satisfies every
/// outstanding predicate, read the way [`matches`] reads a stored entity: `q`,
/// the geo-query and the scope query over every instance it writes.
///
/// A fragment naming no identifier satisfies no selector that constrains one.
#[must_use]
pub fn matches_document(residual: &ResidualFilter, document: &EntityDocument) -> bool {
    matches_selected(residual, document)
}

/// What a residual selection reads of an entity beyond what the query
/// languages read.
trait SelectedEntity<'a>: QueriedEntity<'a> + LocatedEntity<'a> + ScopedEntity<'a> {
    /// The entity's identifier, when it names one.
    fn identifier(self) -> Option<&'a Identifier>;
    /// The entity's types, by expanded name.
    fn type_names(self) -> Vec<&'a str>;
    /// Whether the entity carries a member -- an attribute, `scope` or its
    /// `expiresAt`, by expanded name -- that `admits` accepts.
    fn carries_member(self, admits: impl Fn(&str) -> bool) -> bool;
    /// Whether the entity carries a `scope`.
    fn holds_scope(self) -> bool;
}

impl<'a> SelectedEntity<'a> for &'a EntityState {
    fn identifier(self) -> Option<&'a Identifier> {
        Some(self.id())
    }

    fn type_names(self) -> Vec<&'a str> {
        self.types().as_slice().iter().map(EntityTypeName::as_str).collect()
    }

    fn carries_member(self, admits: impl Fn(&str) -> bool) -> bool {
        self.member_names().any(|name| admits(name.as_str()))
    }

    fn holds_scope(self) -> bool {
        self.scope().is_some()
    }
}

impl<'a> SelectedEntity<'a> for &'a EntityDocument {
    fn identifier(self) -> Option<&'a Identifier> {
        self.id()
    }

    fn type_names(self) -> Vec<&'a str> {
        self.types().iter().map(EntityTypeName::as_str).collect()
    }

    fn carries_member(self, admits: impl Fn(&str) -> bool) -> bool {
        (self.holds_scope() && admits(properties::SCOPE.as_str()))
            || (self.expires_at().and_then(Slot::as_set).is_some() && admits(properties::EXPIRES_AT.as_str()))
            || self.attributes().keys().any(|name| admits(name.as_str()))
    }

    fn holds_scope(self) -> bool {
        self.scope().and_then(Slot::as_set).is_some()
    }
}

/// Whether one entity satisfies every outstanding predicate.
fn matches_selected<'a, E: SelectedEntity<'a>>(residual: &ResidualFilter, entity: E) -> bool {
    // An index's narrowing of the default `location` instances only restates
    // part of `geo`, which is evaluated here over every instance in full.
    let EntitySelection {
        selectors,
        attrs,
        q_expr,
        geo,
        default_location: _,
        scope_q,
        considered,
        projected_members,
    } = residual.selection();
    considers_any_entity(*considered)
        && selectors_match(selectors, entity)
        && attributes_present(attrs, entity)
        && value_filter_matches(q_expr.as_ref(), entity)
        && geo_matches(geo.as_ref(), entity)
        && scope_matches(scope_q.as_ref(), entity)
        && projection_leaves_a_member(projected_members, entity)
}

/// Clause 5.5.14: a request narrowed to an empty identifier set considers no
/// entity, so nothing satisfies it — the same reading the SQL renderer gives it
/// as a constant `FALSE`, kept here so an entity a context source returned
/// cannot enter a page the store would never have answered with.
const fn considers_any_entity(considered: ConsideredEntities) -> bool {
    match considered {
        ConsideredEntities::Selected => true,
        ConsideredEntities::NoEntity => false,
    }
}

/// Clause 5.2.33 union of selectors: an entity matches when any selector admits
/// it, or when no selector constrains the identifier/pattern/type axes.
///
/// Mirrors [`EntitySelection::selectors_constrain`] and the SQL renderer, so a
/// store that pushes the union down and one that finishes it in memory return
/// the same entities.
fn selectors_match<'a, E: SelectedEntity<'a>>(selectors: &[EntitySelector], entity: E) -> bool {
    selectors.is_empty() || selectors.iter().any(|selector| !selector.constrains() || selector_matches(selector, entity))
}

/// Whether one selector's identifier list, pattern and type all hold at once.
fn selector_matches<'a, E: SelectedEntity<'a>>(selector: &EntitySelector, entity: E) -> bool {
    identifier_matches(&selector.ids, selector.id_pattern.as_ref(), entity.identifier()) && type_matches(selector.type_expr.as_ref(), entity)
}

/// Clause 5.7.2 `id` and `idPattern`.
///
/// The pushed-down rendering is `id = ANY(..)` and the PostgreSQL `~` operator.
/// `Pattern` is a Rust `regex`, which agrees with `~` on the IEEE 1003.2
/// extended regular expressions clause 5.2.8 mandates and diverges only on
/// engine extensions no conforming client may write. An entity naming no
/// identifier satisfies neither once either constrains it.
fn identifier_matches(ids: &[Identifier], id_pattern: Option<&Pattern>, id: Option<&Identifier>) -> bool {
    if ids.is_empty() && id_pattern.is_none() {
        return true;
    }
    id.is_some_and(|id| (ids.is_empty() || ids.contains(id)) && id_pattern.is_none_or(|pattern| pattern.is_match(id.as_str())))
}

/// Clause 4.17 type selection.
fn type_matches<'a, E: SelectedEntity<'a>>(type_expr: Option<&TypeExpr>, entity: E) -> bool {
    type_expr.is_none_or(|type_expr| type_expr.matches(&entity.type_names()))
}

/// Clause 5.7.2 `attrs`: the entity must carry at least one attribute named.
///
/// A disjunction, not a conjunction. Table 6.4.3.2-1 is explicit: "only Entities
/// that contain at least one of the Attributes in attrs are to be included in
/// the response". The pushed-down rendering is a disjunction for the same
/// reason, and the two must agree or a store that declines `attrs` returns a
/// different answer from one that does not. Clause 5.5.9 lets the list name
/// `scope` and the entity's `expiresAt` as well as an attribute.
fn attributes_present<'a, E: SelectedEntity<'a>>(attrs: &[IriBuf], entity: E) -> bool {
    attrs.is_empty() || entity.carries_member(|name| attrs.iter().any(|iri| iri.as_str() == name))
}

/// Clause 4.9 `q`.
///
/// A store compiles every `rel{…}` hop of a query's `q` itself, reading the
/// linked entity where it lies, so a hop reaches nothing in memory here.
fn value_filter_matches<'a, E: SelectedEntity<'a>>(q_expr: Option<&Expr>, entity: E) -> bool {
    q_expr.is_none_or(|expr| evaluate(expr, entity, NoLinkedEntities))
}

/// Clause 4.10 `georel`.
fn geo_matches<'a, E: SelectedEntity<'a>>(geo: Option<&GeoQuery>, entity: E) -> bool {
    geo.is_none_or(|geo| geo.matches_entity(entity))
}

/// Clause 4.18 `scopeQ`.
fn scope_matches<'a, E: SelectedEntity<'a>>(scope_q: Option<&ScopeQuery>, entity: E) -> bool {
    scope_q.is_none_or(|scope_q| scope_q.matches_entity(entity))
}

/// Clause 4.21 read as a selection criterion: the projection leaves the entity
/// a member to answer with (clauses 5.7.2.5 and 5.7.4.5).
///
/// The pushed-down rendering reads the same names off the entity's recorded
/// member names, so the two agree on which keys are Attributes.
fn projection_leaves_a_member<'a, E: SelectedEntity<'a>>(projected: &ProjectedMembers, entity: E) -> bool {
    match projected {
        ProjectedMembers::Unconditional => true,
        ProjectedMembers::AnyOf { scope, .. } => keeps_scope(*scope, entity) || entity.carries_member(|name| projected.names().any(|named| named == name)),
        ProjectedMembers::AnyBesides { scope, .. } => keeps_scope(*scope, entity) || entity.carries_member(|name| projected.names().all(|named| named != name)),
    }
}

/// Whether the entity keeps a `scope` member through the projection.
fn keeps_scope<'a, E: SelectedEntity<'a>>(scope: ScopeRetention, entity: E) -> bool {
    match scope {
        ScopeRetention::Kept => entity.holds_scope(),
        ScopeRetention::Removed => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::{query_residual::ResidualFilter, residual_filter::matches};
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_query::{
        entity_selection::{ConsideredEntities, EntitySelection, EntitySelector},
        filter::{expression::Expr, parser},
        geo::query::GeoQuery,
        pattern::Pattern,
        projection::{compiled::Projection, projected_members::ProjectedMembers, term::ProjectionTerm},
        scope::query::ScopeQuery,
        type_selection::grammar::TypeExpr,
    };
    use serde_json::{Value, json};
    use std::str::FromStr;

    /// Whether `residual` admits `state`.
    fn admits(residual: &ResidualFilter, state: &EntityState) -> bool {
        matches(residual, state)
    }

    /// The stored state an expanded document decodes to.
    fn state(document: &Value) -> EntityState {
        EntityState::from_slice(&serde_json::to_vec(document).expect("JSON")).expect("a stored state")
    }

    /// A Point geometry node at `longitude`, `latitude`.
    fn point(longitude: f64, latitude: f64) -> Value {
        json!({"@type": ["https://purl.org/geojson/vocab#Point"], "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": longitude}, {"@value": latitude}]}]})
    }

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";
    const TEMPERATURE: &str = "https://example.com/temperature";
    const BUILDING: &str = "https://example.com/Building";
    const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";
    const GEO_PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/GeoProperty";

    fn q(raw: &str) -> Expr {
        parser::parse(raw).expect("valid q filter")
    }

    fn entity(temperature: f64) -> EntityState {
        state(&json!([{
            "@id": "urn:ngsi-ld:Building:001",
            "@type": [BUILDING],
            TEMPERATURE: [{"@type": [PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": temperature}]}]
        }]))
    }

    fn scoped(scopes: &[&str]) -> EntityState {
        state(&json!([{
            "@id": "urn:ngsi-ld:Building:001",
            "@type": [BUILDING],
            "https://uri.etsi.org/ngsi-ld/scope": scopes.iter().map(|s| json!({"@value": s})).collect::<Vec<Value>>()
        }]))
    }

    fn located(longitude: f64, latitude: f64) -> EntityState {
        state(&json!([{
            "@id": "urn:ngsi-ld:Building:001",
            "@type": [BUILDING],
            LOCATION: [{"@type": [GEO_PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [point(longitude, latitude)]}]
        }]))
    }

    fn near_berlin() -> GeoQuery {
        GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
                min_distance: None,
            },
            geometry: Geometry::from_geojson_value(&json!({"type": "Point", "coordinates": [13.4, 52.5]})).expect("valid geometry"),
            geoproperty: LOCATION.parse().expect("valid attribute name"),
        }
    }

    #[test]
    fn an_empty_residual_admits_every_entity() {
        let residual = ResidualFilter::default();
        assert!(residual.is_empty());
        assert!(admits(&residual, &entity(20.0)));
    }

    #[test]
    fn a_residual_q_filter_excludes_an_entity_the_store_did_not_exclude() {
        // Clause 4.9: `temperature>25` selects only instances above the bound.
        let residual = ResidualFilter::over(EntitySelection::builder().q_expr(Some(q("\"https://example.com/temperature\">25"))).build());
        assert!(!residual.is_empty());
        assert!(!admits(&residual, &entity(20.0)));
        assert!(admits(&residual, &entity(30.0)));
    }

    #[test]
    fn the_residual_q_pass_compares_at_the_bound_the_way_clause_4_9_states_it() {
        // Clause 4.9 `>` is strict: the bound itself is not above the bound,
        // anything past it is.
        let residual = ResidualFilter::over(EntitySelection::builder().q_expr(Some(q("\"https://example.com/temperature\">25"))).build());
        for (temperature, expected) in [(0.0, false), (25.0, false), (25.5, true), (100.0, true)] {
            pretty_assertions::assert_eq!(admits(&residual, &entity(temperature)), expected, "verdict at {temperature}");
        }
    }

    #[test]
    fn a_residual_geo_filter_excludes_an_entity_outside_the_reference_geometry() {
        // Clause 4.10: `near;maxDistance==1000` about the reference point.
        let residual = ResidualFilter::over(EntitySelection::builder().geo(Some(near_berlin())).build());
        assert!(admits(&residual, &located(13.4, 52.5)));
        assert!(!admits(&residual, &located(13.4, 53.5)));
    }

    #[test]
    fn a_residual_geo_filter_excludes_an_entity_that_carries_no_such_geoproperty() {
        // Clause 4.10 relates two geometries; an absent one relates to nothing.
        let residual = ResidualFilter::over(EntitySelection::builder().geo(Some(near_berlin())).build());
        assert!(!admits(&residual, &entity(20.0)));
    }

    #[test]
    fn a_residual_geo_filter_reads_a_geoproperty_written_as_a_json_string() {
        // Clause 4.7 permits the GeoJSON document to arrive as a string.
        let residual = ResidualFilter::over(EntitySelection::builder().geo(Some(near_berlin())).build());
        let encoded = state(&json!([{
            "@id": "urn:ngsi-ld:Building:001",
            "@type": [BUILDING],
            LOCATION: [{"@type": [GEO_PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "{\"type\":\"Point\",\"coordinates\":[13.4,52.5]}"}]}]
        }]));
        assert!(admits(&residual, &encoded));
    }

    #[test]
    fn a_residual_scope_filter_reads_the_scopes_off_the_payload() {
        // Clause 4.18: `#` matches one or more remaining levels.
        let residual = ResidualFilter::over(
            EntitySelection::builder()
                .scope_q(Some(ScopeQuery::parse("/A/#").expect("valid scopeQ")))
                .build(),
        );
        assert!(admits(&residual, &scoped(&["/A/B/C"])));
        assert!(!admits(&residual, &scoped(&["/B"])));
    }

    #[test]
    fn a_residual_type_filter_excludes_an_entity_of_another_type() {
        // Clause 4.17 type selection over expanded type IRIs.
        let residual = ResidualFilter::over(
            EntitySelection::builder()
                .selectors(vec![
                    EntitySelector::builder()
                        .type_expr(Some(TypeExpr::Type(BUILDING.parse().expect("valid IRI"))))
                        .build(),
                ])
                .build(),
        );
        assert!(admits(&residual, &entity(20.0)));
        let vehicle = state(&json!([{"@id": "urn:ngsi-ld:Vehicle:001", "@type": ["https://example.com/Vehicle"]}]));
        assert!(!admits(&residual, &vehicle));
    }

    #[test]
    fn a_residual_attribute_filter_admits_an_entity_carrying_any_named_attribute() {
        // Table 6.4.3.2-1: "only Entities that contain at least one of the
        // Attributes in attrs are to be included in the response". The entity
        // carries `temperature` and not `humidity`, and is admitted.
        let residual = ResidualFilter::over(
            EntitySelection::builder()
                .attrs(vec![
                    TEMPERATURE.parse().expect("valid IRI"),
                    "https://example.com/humidity".parse().expect("valid IRI"),
                ])
                .build(),
        );
        assert!(admits(&residual, &entity(20.0)));
    }

    #[test]
    fn a_residual_attribute_filter_excludes_an_entity_carrying_none_of_them() {
        let residual = ResidualFilter::over(
            EntitySelection::builder()
                .attrs(vec![
                    "https://example.com/humidity".parse().expect("valid IRI"),
                    "https://example.com/pressure".parse().expect("valid IRI"),
                ])
                .build(),
        );
        assert!(!admits(&residual, &entity(20.0)));
    }

    #[test]
    fn a_residual_id_pattern_excludes_an_identifier_it_does_not_match() {
        let residual = ResidualFilter::over(
            EntitySelection::builder()
                .selectors(vec![
                    EntitySelector::builder()
                        .id_pattern(Some(Pattern::parse("^urn:ngsi-ld:Vehicle:").expect("valid pattern")))
                        .build(),
                ])
                .build(),
        );
        assert!(!admits(&residual, &entity(20.0)));
    }

    #[test]
    fn a_residual_identifier_list_excludes_an_identifier_outside_it() {
        let residual = ResidualFilter::over(
            EntitySelection::builder()
                .selectors(vec![
                    EntitySelector::builder()
                        .ids(vec!["urn:ngsi-ld:Building:002".parse().expect("valid URI")])
                        .build(),
                ])
                .build(),
        );
        assert!(!admits(&residual, &entity(20.0)));
    }

    #[test]
    fn every_outstanding_predicate_must_hold_at_once() {
        let residual = ResidualFilter::over(
            EntitySelection::builder()
                .q_expr(Some(q("\"https://example.com/temperature\"<25")))
                .scope_q(Some(ScopeQuery::parse("/Nowhere").expect("valid scopeQ")))
                .build(),
        );
        // The `q` half holds and the `scopeQ` half does not.
        assert!(!admits(&residual, &entity(20.0)));
    }

    #[test]
    fn a_union_of_selectors_admits_only_the_pairings_a_selector_named() {
        // Clause 5.2.33: `{id:A,type:T1}` OR `{id:B,type:T2}` admits A-as-T1 and
        // B-as-T2, but not the cross pairing A-as-T2 that neither selector named.
        let entity = |id: &str, type_iri: &str| state(&json!([{"@id": id, "@type": [type_iri]}]));
        let residual = ResidualFilter::over(
            EntitySelection::builder()
                .selectors(vec![
                    EntitySelector::builder()
                        .ids(vec!["urn:ngsi-ld:Building:001".parse().expect("valid URI")])
                        .type_expr(Some(TypeExpr::Type("https://example.com/Office".parse().expect("valid IRI"))))
                        .build(),
                    EntitySelector::builder()
                        .ids(vec!["urn:ngsi-ld:Building:002".parse().expect("valid URI")])
                        .type_expr(Some(TypeExpr::Type("https://example.com/Warehouse".parse().expect("valid IRI"))))
                        .build(),
                ])
                .build(),
        );
        assert!(admits(&residual, &entity("urn:ngsi-ld:Building:001", "https://example.com/Office")));
        assert!(admits(&residual, &entity("urn:ngsi-ld:Building:002", "https://example.com/Warehouse")));
        assert!(!admits(&residual, &entity("urn:ngsi-ld:Building:001", "https://example.com/Warehouse")));
        assert!(!admits(&residual, &entity("urn:ngsi-ld:Building:002", "https://example.com/Office")));
    }

    /// One entity carrying every member the seven predicate kinds address.
    fn conformance_entity() -> EntityState {
        state(&json!([{
            "@id": "urn:ngsi-ld:Building:001",
            "@type": [BUILDING],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Berlin/Mitte"}],
            TEMPERATURE: [{"@type": [PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 30.0}]}],
            LOCATION: [{"@type": [GEO_PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [point(13.4, 52.5)]}]
        }]))
    }

    /// One clause of the specification, the selection that states it, and the
    /// verdict the reference reading requires for [`conformance_entity`].
    type ConformanceCase = (&'static str, EntitySelection, bool);

    /// The clause 5.7.2 `id` and clause 5.2.8 `idPattern` readings.
    fn identifier_selection_cases() -> Vec<ConformanceCase> {
        vec![
            (
                "clause 5.7.2 id: the list names this entity",
                EntitySelection::builder()
                    .selectors(vec![
                        EntitySelector::builder()
                            .ids(vec!["urn:ngsi-ld:Building:001".parse().expect("valid URI")])
                            .build(),
                    ])
                    .build(),
                true,
            ),
            (
                "clause 5.7.2 id: the list names another entity",
                EntitySelection::builder()
                    .selectors(vec![
                        EntitySelector::builder()
                            .ids(vec!["urn:ngsi-ld:Building:002".parse().expect("valid URI")])
                            .build(),
                    ])
                    .build(),
                false,
            ),
            (
                "clause 5.5.14: a request narrowed to an empty identifier set considers no entity",
                EntitySelection::builder().considered(ConsideredEntities::NoEntity).build(),
                false,
            ),
            (
                "clause 5.5.14: a request under no EntityMap considers whatever it selects",
                EntitySelection::builder().considered(ConsideredEntities::Selected).build(),
                true,
            ),
            (
                "clause 5.2.8 idPattern: the expression matches the identifier",
                EntitySelection::builder()
                    .selectors(vec![
                        EntitySelector::builder()
                            .id_pattern(Some(Pattern::parse("^urn:ngsi-ld:Building:").expect("valid pattern")))
                            .build(),
                    ])
                    .build(),
                true,
            ),
            (
                "clause 5.2.8 idPattern: the expression does not match",
                EntitySelection::builder()
                    .selectors(vec![
                        EntitySelector::builder()
                            .id_pattern(Some(Pattern::parse("^urn:ngsi-ld:Vehicle:").expect("valid pattern")))
                            .build(),
                    ])
                    .build(),
                false,
            ),
        ]
    }

    /// The clause 4.17 type-expression readings.
    fn type_selection_cases() -> Vec<ConformanceCase> {
        vec![
            (
                "clause 4.17 type: the entity carries the named type",
                EntitySelection::builder()
                    .selectors(vec![
                        EntitySelector::builder()
                            .type_expr(Some(TypeExpr::Type(BUILDING.parse().expect("valid IRI"))))
                            .build(),
                    ])
                    .build(),
                true,
            ),
            (
                "clause 4.17 type: a conjunction the entity only half satisfies",
                EntitySelection::builder()
                    .selectors(vec![
                        EntitySelector::builder()
                            .type_expr(Some(TypeExpr::And(vec![
                                TypeExpr::Type(BUILDING.parse().expect("valid IRI")),
                                TypeExpr::Type("https://example.com/Vehicle".parse().expect("valid IRI")),
                            ])))
                            .build(),
                    ])
                    .build(),
                false,
            ),
        ]
    }

    /// The Table 6.4.3.2-1 `attrs` readings.
    fn attribute_selection_cases() -> Vec<ConformanceCase> {
        vec![
            (
                "Table 6.4.3.2-1 attrs: the entity carries at least one of them",
                EntitySelection::builder()
                    .attrs(vec![
                        TEMPERATURE.parse().expect("valid IRI"),
                        "https://example.com/humidity".parse().expect("valid IRI"),
                    ])
                    .build(),
                true,
            ),
            (
                "Table 6.4.3.2-1 attrs: the entity carries none of them",
                EntitySelection::builder()
                    .attrs(vec!["https://example.com/humidity".parse().expect("valid IRI")])
                    .build(),
                false,
            ),
        ]
    }

    /// The clause 4.9 `q` and clause 4.10 `georel` readings.
    fn value_and_geo_selection_cases() -> Vec<ConformanceCase> {
        vec![
            (
                "clause 4.9 q: the value satisfies the comparison",
                EntitySelection::builder().q_expr(Some(q("\"https://example.com/temperature\">25"))).build(),
                true,
            ),
            (
                "clause 4.9 q: the value does not",
                EntitySelection::builder().q_expr(Some(q("\"https://example.com/temperature\"<25"))).build(),
                false,
            ),
            (
                "clause 4.10 georel: the geometry is inside the reference distance",
                EntitySelection::builder().geo(Some(near_berlin())).build(),
                true,
            ),
        ]
    }

    /// The clause 4.18 and clause 4.19 `scopeQ` readings.
    fn scope_selection_cases() -> Vec<ConformanceCase> {
        vec![
            (
                "clause 4.19 scopeQ: a prefix multi-level wildcard matches a descendant of its anchor",
                EntitySelection::builder()
                    .scope_q(Some(ScopeQuery::parse("/Berlin/#").expect("valid scopeQ")))
                    .build(),
                true,
            ),
            (
                "clause 4.19 scopeQ Example 2: a prefix multi-level wildcard also matches its anchor itself",
                EntitySelection::builder()
                    .scope_q(Some(ScopeQuery::parse("/Berlin/Mitte/#").expect("valid scopeQ")))
                    .build(),
                true,
            ),
            (
                "clause 4.18 scopeQ: a literal path matches exactly",
                EntitySelection::builder()
                    .scope_q(Some(ScopeQuery::parse("/Berlin/Mitte").expect("valid scopeQ")))
                    .build(),
                true,
            ),
        ]
    }

    /// A selection carrying only what `projection` requires of an entity.
    fn projected(projection: &Projection) -> EntitySelection {
        EntitySelection::builder().projected_members(ProjectedMembers::of(projection)).build()
    }

    fn attribute_term(iri: &str) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: iri.parse().expect("valid IRI"),
            linked: Vec::new(),
        }
    }

    /// The clause 4.21 projection read as a selection (clauses 5.7.2.5 and
    /// 5.7.4.5, ETSI TP `019_17_01`).
    fn projected_member_cases() -> Vec<ConformanceCase> {
        vec![
            (
                "clause 4.21 pick: the entity carries the picked attribute",
                projected(&Projection::builder().pick(vec![attribute_term(TEMPERATURE)]).build()),
                true,
            ),
            (
                "clause 4.21 pick: the entity carries none of the picked attributes",
                projected(&Projection::builder().pick(vec![attribute_term("https://example.com/humidity")]).build()),
                false,
            ),
            (
                "clause 4.21 pick: a picked scope keeps the entity through its scope",
                projected(&Projection::builder().pick(vec![ProjectionTerm::Scope]).build()),
                true,
            ),
            (
                "clause 4.21 omit: every member the entity has is omitted",
                projected(
                    &Projection::builder()
                        .omit(vec![
                            ProjectionTerm::Id,
                            ProjectionTerm::Type,
                            ProjectionTerm::Scope,
                            attribute_term(TEMPERATURE),
                            attribute_term(LOCATION),
                        ])
                        .build(),
                ),
                false,
            ),
            (
                "clause 4.21 omit: an attribute the omit does not name survives",
                projected(
                    &Projection::builder()
                        .omit(vec![
                            ProjectionTerm::Id,
                            ProjectionTerm::Type,
                            ProjectionTerm::Scope,
                            attribute_term(TEMPERATURE),
                        ])
                        .build(),
                ),
                true,
            ),
        ]
    }

    #[test]
    fn every_predicate_kind_reads_its_clause_the_way_the_specification_states_it() {
        // One table over every predicate an `EntitySelection` can carry,
        // including the five no `StorageProfile` axis can produce today. The expected
        // verdicts are read off the specification rather than off the
        // implementation, which is what makes this a check and not a restatement
        // -- an earlier reading of `attrs` as a conjunction passed every test
        // that restated it.
        let entity = conformance_entity();
        let cases = identifier_selection_cases()
            .into_iter()
            .chain(type_selection_cases())
            .chain(attribute_selection_cases())
            .chain(value_and_geo_selection_cases())
            .chain(scope_selection_cases())
            .chain(projected_member_cases());

        for (clause, selection, expected) in cases {
            pretty_assertions::assert_eq!(admits(&ResidualFilter::over(selection), &entity), expected, "{clause}");
        }
    }
}
