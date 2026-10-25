//! The filters a request places on registrations beside clause 5.12 matching
//! (clauses 4.9, 4.10, 4.19, 5.10.2.4 and 5.11.2.4), decided once for every
//! reader: a forwarded read, a subscription to registrations and their tests.

use crate::registration::ContextSourceRegistration;
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

/// The filters one request places on registrations, each optional.
#[derive(Debug, Clone, Copy)]
pub struct RegistrationFilters<'a> {
    /// The Scope query, matched against the registration's `scope`.
    pub scope_q: Option<&'a ScopeQuery>,
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
        if !temporal || self.scope_q.is_some_and(|scope_q| !coverage.admits_scope_query(scope_q)) || self.geo.is_some_and(|geo| !coverage.admits_geo_query(geo))
        {
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::registration::{
        ContextSourceRegistration,
        selection::{RegistrationFilters, TemporalPartition},
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
        scope_q: None,
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
    fn a_scope_query_admits_only_a_registration_whose_scope_it_matches() {
        let madrid = ScopeQuery::parse("/Madrid/#").expect("a valid scope query");
        let filters = RegistrationFilters {
            scope_q: Some(&madrid),
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
