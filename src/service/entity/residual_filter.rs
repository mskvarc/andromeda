//! The predicates a store declined to apply, and the in-memory pass that
//! finishes them.
//!
//! Every store port returns rows. None of them can be assumed to have applied
//! every predicate it was given: a backend with no PostGIS cannot answer
//! `georel=near`, and one with no `ltree` cannot answer a `scopeQ` wildcard.
//! Such a store says so, and the policy layer finishes the job here rather than
//! silently returning unfiltered results.

use crate::service::geo_filter::matches_geo_query;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::expanded::{
    entity_member::{extract_id, extract_scopes, extract_types},
    node::expanded_object_ref,
};
use andromeda_ngsi_ld_query::{
    entity_selection::EntitySelection,
    filter::{eval, expression::Expr},
    geo::query::GeoQuery,
    pattern::Pattern,
    scope::query::ScopeQuery,
    type_selection::grammar::TypeExpr,
};
use iri_rs::IriBuf;
use serde_json::Value;
use strum::Display;

/// One kind of predicate a store may decline.
///
/// Spelled as the NGSI-LD query parameter it comes from (clause 5.7.2), because
/// the only thing this vocabulary is used for is telling a client which part of
/// its request could not be answered.
///
/// All seven members of an [`EntitySelection`] are named, not just the three
/// [`StorageProfile`] has axes for today. The residual *is* a selection, so
/// [`ResidualFilter::kinds`] must be able to name whatever a selection can
/// carry; a shorter enum would make some selections unnameable. Four of the
/// seven therefore have no production constructor, and the in-memory readings
/// of those four are held to the pushed-down renderings by the profile-driven
/// conformance tests rather than by a live caller.
///
/// [`StorageProfile`]: crate::service::entity::storage_profile::StorageProfile
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum ResidualPredicate {
    /// The `id` list.
    #[strum(serialize = "id")]
    Identifier,
    /// The `idPattern` regular expression.
    #[strum(serialize = "idPattern")]
    IdentifierPattern,
    /// The `type` selection expression (clause 4.17).
    #[strum(serialize = "type")]
    EntityType,
    /// The `attrs` list.
    #[strum(serialize = "attrs")]
    Attributes,
    /// The `q` filter (clause 4.9).
    #[strum(serialize = "q")]
    ValueFilter,
    /// The `georel` / `geometry` / `coordinates` triple (clause 4.10).
    #[strum(serialize = "georel")]
    GeoQuery,
    /// The `scopeQ` expression (clause 4.18).
    #[strum(serialize = "scopeQ")]
    ScopeQuery,
}

/// The subset of an [`EntitySelection`] a store did not apply.
///
/// A newtype over the selection rather than a second seven-field struct: the
/// residual is drawn from exactly the same vocabulary of predicates, and
/// restating them would be two definitions of one concept. The wrapper exists
/// so a call site cannot confuse what was asked for with what is left over.
#[derive(Debug, Default)]
pub struct ResidualFilter(EntitySelection);

impl ResidualFilter {
    /// A residual over the predicates in `selection`.
    pub const fn over(selection: EntitySelection) -> ResidualFilter {
        ResidualFilter(selection)
    }

    /// Whether the store applied every predicate, leaving nothing to finish.
    pub fn is_empty(&self) -> bool {
        self.kinds().is_empty()
    }

    /// Remove from `selection` every predicate this residual carries.
    ///
    /// What is left is the query a store actually issues: asking it to apply a
    /// predicate it has already said it cannot would either fail or, worse,
    /// silently render something weaker.
    pub fn strip_from(&self, selection: &mut EntitySelection) {
        let EntitySelection {
            ids,
            id_pattern,
            type_expr,
            attrs,
            q_expr,
            geo,
            scope_q,
        } = &self.0;
        if !ids.is_empty() {
            selection.ids.clear();
        }
        if id_pattern.is_some() {
            selection.id_pattern = None;
        }
        if type_expr.is_some() {
            selection.type_expr = None;
        }
        if !attrs.is_empty() {
            selection.attrs.clear();
        }
        if q_expr.is_some() {
            selection.q_expr = None;
        }
        if geo.is_some() {
            selection.geo = None;
        }
        if scope_q.is_some() {
            selection.scope_q = None;
        }
    }

    /// Which kinds of predicate are outstanding, in the order they are
    /// evaluated.
    pub fn kinds(&self) -> Vec<ResidualPredicate> {
        let EntitySelection {
            ids,
            id_pattern,
            type_expr,
            attrs,
            q_expr,
            geo,
            scope_q,
        } = &self.0;
        let mut kinds = Vec::new();
        if !ids.is_empty() {
            kinds.push(ResidualPredicate::Identifier);
        }
        if id_pattern.is_some() {
            kinds.push(ResidualPredicate::IdentifierPattern);
        }
        if type_expr.is_some() {
            kinds.push(ResidualPredicate::EntityType);
        }
        if !attrs.is_empty() {
            kinds.push(ResidualPredicate::Attributes);
        }
        if q_expr.is_some() {
            kinds.push(ResidualPredicate::ValueFilter);
        }
        if geo.is_some() {
            kinds.push(ResidualPredicate::GeoQuery);
        }
        if scope_q.is_some() {
            kinds.push(ResidualPredicate::ScopeQuery);
        }
        kinds
    }

    /// Whether one expanded entity satisfies every outstanding predicate.
    ///
    /// Every predicate must hold at once, which is the clause 4.9 conjunction
    /// the store would have rendered as `AND`.
    pub fn matches(&self, expanded: &Value) -> bool {
        let EntitySelection {
            ids,
            id_pattern,
            type_expr,
            attrs,
            q_expr,
            geo,
            scope_q,
        } = &self.0;
        identifier_matches(ids, id_pattern.as_ref(), expanded)
            && type_matches(type_expr.as_ref(), expanded)
            && attributes_present(attrs, expanded)
            && value_filter_matches(q_expr.as_ref(), expanded)
            && geo_matches(geo.as_ref(), expanded)
            && scope_matches(scope_q.as_ref(), expanded)
    }
}

/// Clause 5.7.2 `id` and `idPattern`.
///
/// The pushed-down rendering is `id = ANY(..)` and the PostgreSQL `~` operator.
/// `Pattern` is a Rust `regex`, which agrees with `~` on the IEEE 1003.2
/// extended regular expressions clause 5.2.8 mandates and diverges only on
/// engine extensions no conforming client may write.
fn identifier_matches(ids: &[Uri], id_pattern: Option<&Pattern>, expanded: &Value) -> bool {
    if ids.is_empty() && id_pattern.is_none() {
        return true;
    }
    let Some(id) = extract_id(expanded) else {
        return false;
    };
    if !ids.is_empty() && !ids.iter().any(|wanted| AsRef::<str>::as_ref(wanted) == id) {
        return false;
    }
    id_pattern.is_none_or(|pattern| pattern.is_match(id))
}

/// Clause 4.17 type selection.
fn type_matches(type_expr: Option<&TypeExpr>, expanded: &Value) -> bool {
    let Some(type_expr) = type_expr else {
        return true;
    };
    let types = extract_types(expanded);
    let names: Vec<&str> = types.iter().map(EntityTypeName::as_str).collect();
    type_expr.matches(&names)
}

/// Clause 5.7.2 `attrs`: the entity must carry at least one attribute named.
///
/// A disjunction, not a conjunction. Table 6.4.3.2-1 is explicit: "only Entities
/// that contain at least one of the Attributes in attrs are to be included in
/// the response". The pushed-down rendering is a disjunction for the same
/// reason, and the two must agree or a store that declines `attrs` returns a
/// different answer from one that does not.
fn attributes_present(attrs: &[IriBuf], expanded: &Value) -> bool {
    if attrs.is_empty() {
        return true;
    }
    expanded_object_ref(expanded).is_some_and(|obj| attrs.iter().any(|iri| obj.contains_key(iri.as_str())))
}

/// Clause 4.9 `q`.
fn value_filter_matches(q_expr: Option<&Expr>, expanded: &Value) -> bool {
    q_expr.is_none_or(|expr| eval::evaluate(expr, expanded))
}

/// Clause 4.10 `georel`.
fn geo_matches(geo: Option<&GeoQuery>, expanded: &Value) -> bool {
    geo.is_none_or(|geo| matches_geo_query(geo, expanded))
}

/// Clause 4.18 `scopeQ`.
fn scope_matches(scope_q: Option<&ScopeQuery>, expanded: &Value) -> bool {
    scope_q.is_none_or(|scope_q| scope_q.matches(&extract_scopes(expanded)))
}

#[cfg(test)]
mod tests {
    use crate::service::entity::residual_filter::{ResidualFilter, ResidualPredicate};
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        filter::{eval, expression::Expr, parser},
        geo::query::GeoQuery,
        pattern::Pattern,
        scope::query::ScopeQuery,
        type_selection::grammar::TypeExpr,
    };
    use serde_json::{Value, json};

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";
    const TEMPERATURE: &str = "https://example.com/temperature";
    const BUILDING: &str = "https://example.com/Building";

    fn q(raw: &str) -> Expr {
        parser::parse(raw).expect("valid q filter")
    }

    fn entity(temperature: f64) -> Value {
        json!([{
            "@id": "urn:ngsi-ld:Building:001",
            "@type": [BUILDING],
            TEMPERATURE: [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": temperature}]}]
        }])
    }

    fn scoped(scopes: &[&str]) -> Value {
        json!([{
            "@id": "urn:ngsi-ld:Building:001",
            "@type": [BUILDING],
            "https://uri.etsi.org/ngsi-ld/scope": scopes.iter().map(|s| json!({"@value": s})).collect::<Vec<Value>>()
        }])
    }

    fn located(longitude: f64, latitude: f64) -> Value {
        json!([{
            "@id": "urn:ngsi-ld:Building:001",
            "@type": [BUILDING],
            LOCATION: [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": {"type": "Point", "coordinates": [longitude, latitude]}}]}]
        }])
    }

    fn near_berlin() -> GeoQuery {
        GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(1000.0),
                min_distance: None,
            },
            geometry: Geometry::from_geojson_value(&json!({"type": "Point", "coordinates": [13.4, 52.5]})).expect("valid geometry"),
            geoproperty_iri: LOCATION.parse().expect("valid IRI"),
        }
    }

    #[test]
    fn an_empty_residual_admits_every_entity() {
        let residual = ResidualFilter::default();
        assert!(residual.is_empty());
        assert!(residual.matches(&entity(20.0)));
    }

    #[test]
    fn a_residual_q_filter_excludes_an_entity_the_store_did_not_exclude() {
        // Clause 4.9: `temperature>25` selects only instances above the bound.
        let residual = ResidualFilter::over(EntitySelection::builder().q_expr(Some(q("\"https://example.com/temperature\">25"))).build());
        assert!(!residual.is_empty());
        assert!(!residual.matches(&entity(20.0)));
        assert!(residual.matches(&entity(30.0)));
    }

    #[test]
    fn the_residual_q_pass_returns_what_the_pushed_down_predicate_would_have() {
        // The residual is the same expression the store would have rendered, so
        // the two readings of clause 4.9 must agree entity for entity.
        let expr = q("\"https://example.com/temperature\">25");
        let residual = ResidualFilter::over(EntitySelection::builder().q_expr(Some(q("\"https://example.com/temperature\">25"))).build());
        for temperature in [0.0, 25.0, 25.5, 100.0] {
            let payload = entity(temperature);
            pretty_assertions::assert_eq!(residual.matches(&payload), eval::evaluate(&expr, &payload), "disagreement at {temperature}");
        }
    }

    #[test]
    fn a_residual_geo_filter_excludes_an_entity_outside_the_reference_geometry() {
        // Clause 4.10: `near;maxDistance==1000` about the reference point.
        let residual = ResidualFilter::over(EntitySelection::builder().geo(Some(near_berlin())).build());
        assert!(residual.matches(&located(13.4, 52.5)));
        assert!(!residual.matches(&located(13.4, 53.5)));
    }

    #[test]
    fn a_residual_geo_filter_excludes_an_entity_that_carries_no_such_geoproperty() {
        // Clause 4.10 relates two geometries; an absent one relates to nothing.
        let residual = ResidualFilter::over(EntitySelection::builder().geo(Some(near_berlin())).build());
        assert!(!residual.matches(&entity(20.0)));
    }

    #[test]
    fn a_residual_geo_filter_reads_a_geoproperty_written_as_a_json_string() {
        // Clause 4.7 permits the GeoJSON document to arrive as a string.
        let residual = ResidualFilter::over(EntitySelection::builder().geo(Some(near_berlin())).build());
        let payload = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "{\"type\":\"Point\",\"coordinates\":[13.4,52.5]}"}]}]
        }]);
        assert!(residual.matches(&payload));
    }

    #[test]
    fn a_residual_scope_filter_reads_the_scopes_off_the_payload() {
        // Clause 4.18: `#` matches one or more remaining levels.
        let residual = ResidualFilter::over(
            EntitySelection::builder()
                .scope_q(Some(ScopeQuery::parse("/A/#").expect("valid scopeQ")))
                .build(),
        );
        assert!(residual.matches(&scoped(&["/A/B/C"])));
        assert!(!residual.matches(&scoped(&["/B"])));
    }

    #[test]
    fn a_residual_type_filter_excludes_an_entity_of_another_type() {
        // Clause 4.17 type selection over expanded type IRIs.
        let residual = ResidualFilter::over(
            EntitySelection::builder()
                .type_expr(Some(TypeExpr::Type(BUILDING.parse().expect("valid IRI"))))
                .build(),
        );
        assert!(residual.matches(&entity(20.0)));
        let vehicle = json!([{"@id": "urn:ngsi-ld:Vehicle:001", "@type": ["https://example.com/Vehicle"]}]);
        assert!(!residual.matches(&vehicle));
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
        assert!(residual.matches(&entity(20.0)));
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
        assert!(!residual.matches(&entity(20.0)));
    }

    #[test]
    fn a_residual_id_pattern_excludes_an_identifier_it_does_not_match() {
        let residual = ResidualFilter::over(
            EntitySelection::builder()
                .id_pattern(Some(Pattern::parse("^urn:ngsi-ld:Vehicle:").expect("valid pattern")))
                .build(),
        );
        assert!(!residual.matches(&entity(20.0)));
    }

    #[test]
    fn a_residual_identifier_list_excludes_an_identifier_outside_it() {
        let residual = ResidualFilter::over(
            EntitySelection::builder()
                .ids(vec!["urn:ngsi-ld:Building:002".parse().expect("valid URI")])
                .build(),
        );
        assert!(!residual.matches(&entity(20.0)));
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
        assert!(!residual.matches(&entity(20.0)));
    }

    #[test]
    fn an_empty_residual_names_no_outstanding_predicate_kinds() {
        assert!(ResidualFilter::default().kinds().is_empty());
    }

    #[test]
    fn a_residual_names_every_predicate_kind_it_carries() {
        let residual = ResidualFilter::over(
            EntitySelection::builder()
                .q_expr(Some(q("\"https://example.com/temperature\">25")))
                .geo(Some(near_berlin()))
                .scope_q(Some(ScopeQuery::parse("/A").expect("valid scopeQ")))
                .build(),
        );
        pretty_assertions::assert_eq!(
            residual.kinds(),
            vec![ResidualPredicate::ValueFilter, ResidualPredicate::GeoQuery, ResidualPredicate::ScopeQuery]
        );
    }

    /// One entity carrying every member the seven predicate kinds address.
    fn conformance_entity() -> Value {
        json!([{
            "@id": "urn:ngsi-ld:Building:001",
            "@type": [BUILDING],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Berlin/Mitte"}],
            TEMPERATURE: [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 30.0}]}],
            LOCATION: [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": {"type": "Point", "coordinates": [13.4, 52.5]}}]}]
        }])
    }

    #[test]
    fn every_predicate_kind_reads_its_clause_the_way_the_specification_states_it() {
        // One table over all seven members of an `EntitySelection`, including
        // the four no `StorageProfile` axis can produce today. The expected
        // verdicts are read off the specification rather than off the
        // implementation, which is what makes this a check and not a restatement
        // -- an earlier reading of `attrs` as a conjunction passed every test
        // that restated it.
        let entity = conformance_entity();
        let cases: Vec<(&str, EntitySelection, bool)> = vec![
            (
                "clause 5.7.2 id: the list names this entity",
                EntitySelection::builder()
                    .ids(vec!["urn:ngsi-ld:Building:001".parse().expect("valid URI")])
                    .build(),
                true,
            ),
            (
                "clause 5.7.2 id: the list names another entity",
                EntitySelection::builder()
                    .ids(vec!["urn:ngsi-ld:Building:002".parse().expect("valid URI")])
                    .build(),
                false,
            ),
            (
                "clause 5.2.8 idPattern: the expression matches the identifier",
                EntitySelection::builder()
                    .id_pattern(Some(Pattern::parse("^urn:ngsi-ld:Building:").expect("valid pattern")))
                    .build(),
                true,
            ),
            (
                "clause 5.2.8 idPattern: the expression does not match",
                EntitySelection::builder()
                    .id_pattern(Some(Pattern::parse("^urn:ngsi-ld:Vehicle:").expect("valid pattern")))
                    .build(),
                false,
            ),
            (
                "clause 4.17 type: the entity carries the named type",
                EntitySelection::builder()
                    .type_expr(Some(TypeExpr::Type(BUILDING.parse().expect("valid IRI"))))
                    .build(),
                true,
            ),
            (
                "clause 4.17 type: a conjunction the entity only half satisfies",
                EntitySelection::builder()
                    .type_expr(Some(TypeExpr::And(vec![
                        TypeExpr::Type(BUILDING.parse().expect("valid IRI")),
                        TypeExpr::Type("https://example.com/Vehicle".parse().expect("valid IRI")),
                    ])))
                    .build(),
                false,
            ),
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
        ];

        for (clause, selection, expected) in cases {
            pretty_assertions::assert_eq!(ResidualFilter::over(selection).matches(&entity), expected, "{clause}");
        }
    }

    #[test]
    fn a_residual_predicate_is_named_by_the_query_parameter_it_comes_from() {
        pretty_assertions::assert_eq!(ResidualPredicate::ValueFilter.to_string(), "q");
        pretty_assertions::assert_eq!(ResidualPredicate::GeoQuery.to_string(), "georel");
        pretty_assertions::assert_eq!(ResidualPredicate::ScopeQuery.to_string(), "scopeQ");
        pretty_assertions::assert_eq!(ResidualPredicate::IdentifierPattern.to_string(), "idPattern");
    }
}
