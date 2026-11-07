//! The filters a request places on registrations beside clause 5.12 matching
//! (clauses 4.9, 4.10, 4.19, 5.10.2.4 and 5.11.2.4), decided once for every
//! reader: a forwarded read, a subscription to registrations and their tests.

use crate::registration::{ContextSourceRegistration, coverage::RegistrationCoverage};
use andromeda_json_literal::admission::JsonLiteralError;
use andromeda_ngsi_ld_query::{
    csr::compiled::CompiledCsrTemporalFilter,
    filter::{eval::evaluate, expression::Expr, language_matching::LanguageMatching},
    geo::query::GeoQuery,
    queried_entity::linked_entities::NoLinkedEntities,
    scope::query::ScopeQuery,
};

/// Whether the clause 5.10.2.4 temporal partition applies.
///
/// Registration discovery and subscriptions to registrations split
/// registrations into latest-information and interval-bearing ones by the
/// temporal query (clauses 5.10.2.4 and 5.11.2.4). A forwarded read matches by
/// clause 5.12, which has no temporal axis, and contacts every matching
/// registration supporting its operation (clauses 5.7.3.4 and 5.7.4.4).
#[derive(Debug, Clone, Copy)]
pub enum TemporalPartition<'a> {
    /// No partition: a forwarded read.
    NotApplied,
    /// The partition the temporal query selects, or the latest-information side
    /// without one.
    Applied(Option<&'a CompiledCsrTemporalFilter>),
}

/// How the Scope query selects registrations.
///
/// A registration's `scope` lists the Scopes its Context Source has Entities
/// in (Table 5.2.9-1). Registration discovery and subscriptions to
/// registrations match the query against that member (clauses 5.10.2.4 and
/// 5.11.2.4). A forwarded operation matches by clause 5.12, which compares no
/// scope, and sends the Scope query on to the source when entities are not
/// split (clause 4.3.6.7), so the declared Scopes only rule a source out.
#[derive(Debug, Clone, Copy)]
pub enum ScopeFilter<'a> {
    /// No Scope query.
    Unfiltered,
    /// The query must match a declared Scope. A registration declaring none
    /// matches nothing, as an Entity without a scope matches no Scope query
    /// (clause 4.19).
    DeclaredScope(&'a ScopeQuery),
    /// The query must match a declared Scope when the registration declares
    /// any. A registration declaring none may hold an Entity in any Scope, as
    /// one declaring no `location` may hold an Entity anywhere.
    PossibleScope(&'a ScopeQuery),
}

impl ScopeFilter<'_> {
    /// Whether a registration covering `coverage` passes.
    fn admits(self, coverage: &RegistrationCoverage) -> bool {
        match self {
            ScopeFilter::Unfiltered => true,
            ScopeFilter::DeclaredScope(scope_q) => coverage.admits_scope_query(scope_q),
            ScopeFilter::PossibleScope(scope_q) => coverage.scope.is_none() || coverage.admits_scope_query(scope_q),
        }
    }
}

/// The filters one request places on registrations, each optional.
#[derive(Debug, Clone, Copy)]
pub struct RegistrationFilters<'a> {
    /// The Scope query, matched against the registration's `scope`.
    pub scope: ScopeFilter<'a>,
    /// The geoquery, matched against the geometry its `geoproperty` names.
    pub geo: Option<&'a GeoQuery>,
    /// The context source filter, expanded, matched against the Context Source
    /// Properties.
    pub csf: Option<&'a Expr>,
    /// The temporal partition.
    pub temporal: TemporalPartition<'a>,
}

impl RegistrationFilters<'_> {
    /// Whether `registration` passes every filter present.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLiteralError`] when a context source filter is present and
    /// the registration's Context Source Properties do not write as a document
    /// it could read.
    pub fn admits(&self, registration: &ContextSourceRegistration) -> Result<bool, JsonLiteralError> {
        let coverage = &registration.coverage;
        let temporal = match self.temporal {
            TemporalPartition::NotApplied => true,
            TemporalPartition::Applied(temporal_q) => coverage.admits_temporal(temporal_q),
        };
        if !temporal || !self.scope.admits(coverage) || self.geo.is_some_and(|geo| !coverage.admits_geo_query(geo)) {
            return Ok(false);
        }
        let Some(csf) = self.csf else {
            return Ok(true);
        };
        let document = registration.property_members().document()?;
        // Context Source Properties carry no Relationship a hop could cross,
        // and no LanguageProperty a target language could select keys of.
        Ok(evaluate(csf, document.properties(), NoLinkedEntities, LanguageMatching::TagEquality))
    }
}

#[cfg(test)]
mod tests {
    use crate::registration::{
        ContextSourceRegistration,
        selection::{RegistrationFilters, ScopeFilter, TemporalPartition},
    };
    use andromeda_ngsi_ld_query::{filter::parser::parse, scope::query::ScopeQuery};
    use serde_json::{Value, json};

    fn registration(extra: &Value) -> ContextSourceRegistration {
        let mut document = json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:1",
            "type": "ContextSourceRegistration",
            "information": [{"propertyNames": ["https://example.org/speed"]}],
            "endpoint": "https://source.example.com/",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z"
        });
        if let (Some(members), Some(added)) = (document.as_object_mut(), extra.as_object()) {
            members.extend(added.iter().map(|(name, value)| (name.clone(), value.clone()))); // clone: the fixture owns its members
        }
        serde_json::from_value(document).expect("a registration")
    }

    const NONE: RegistrationFilters<'static> = RegistrationFilters {
        scope: ScopeFilter::Unfiltered,
        geo: None,
        csf: None,
        temporal: TemporalPartition::NotApplied,
    };

    #[test]
    fn no_filter_admits_every_registration() {
        assert!(NONE.admits(&registration(&json!({}))).expect("admission decides"));
        assert!(
            NONE.admits(&registration(&json!({"observationInterval": {"startAt": "2020-01-01T00:00:00Z"}})))
                .expect("admission decides"),
            "a forwarded read contacts an interval-bearing registration"
        );
    }

    /// Clauses 5.10.2.4 and 5.11.2.4: without a temporal query the applied
    /// partition keeps only latest-information registrations.
    #[test]
    fn the_applied_partition_without_a_temporal_query_keeps_latest_information_only() {
        let applied = RegistrationFilters {
            temporal: TemporalPartition::Applied(None),
            ..NONE
        };
        assert!(applied.admits(&registration(&json!({}))).expect("admission decides"));
        assert!(
            !applied
                .admits(&registration(&json!({"observationInterval": {"startAt": "2020-01-01T00:00:00Z"}})))
                .expect("admission decides")
        );
    }

    #[test]
    fn a_declared_scope_filter_admits_only_a_registration_whose_scope_it_matches() {
        let madrid = ScopeQuery::parse("/Madrid/#").expect("a valid scope query");
        let filters = RegistrationFilters {
            scope: ScopeFilter::DeclaredScope(&madrid),
            ..NONE
        };
        assert!(
            filters
                .admits(&registration(&json!({"scope": ["/Madrid/Gardens"]})))
                .expect("admission decides")
        );
        assert!(!filters.admits(&registration(&json!({"scope": ["/Berlin"]}))).expect("admission decides"));
        assert!(!filters.admits(&registration(&json!({}))).expect("admission decides"));
    }

    /// Table 5.2.9-1 and clause 5.12: a registration declaring no `scope` may
    /// hold an Entity the forwarded Scope query matches; one declaring other
    /// Scopes cannot.
    #[test]
    fn a_possible_scope_filter_admits_a_matching_or_an_unscoped_registration() {
        let madrid = ScopeQuery::parse("/Madrid/#").expect("a valid scope query");
        let filters = RegistrationFilters {
            scope: ScopeFilter::PossibleScope(&madrid),
            ..NONE
        };
        assert!(
            filters
                .admits(&registration(&json!({"scope": ["/Madrid/Gardens"]})))
                .expect("admission decides")
        );
        assert!(!filters.admits(&registration(&json!({"scope": ["/Berlin"]}))).expect("admission decides"));
        assert!(filters.admits(&registration(&json!({}))).expect("admission decides"));
    }

    /// Clause 4.9: the context source filter reads the registration's own
    /// `mode` and the properties it adds.
    #[test]
    fn a_context_source_filter_reads_the_mode_and_the_added_properties() {
        let exclusive = parse("\"https://uri.etsi.org/ngsi-ld/mode\"==\"exclusive\"").expect("the filter parses");
        let filters = RegistrationFilters { csf: Some(&exclusive), ..NONE };
        assert!(filters.admits(&registration(&json!({"mode": "exclusive"}))).expect("admission decides"));
        assert!(!filters.admits(&registration(&json!({"mode": "inclusive"}))).expect("admission decides"));

        let gold = parse("\"https://example.org/tier\"==\"gold\"").expect("the filter parses");
        let filters = RegistrationFilters { csf: Some(&gold), ..NONE };
        assert!(
            filters
                .admits(&registration(&json!({"https://example.org/tier": "gold"})))
                .expect("admission decides")
        );
        assert!(!filters.admits(&registration(&json!({}))).expect("admission decides"));
    }
}
