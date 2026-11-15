//! The canonical digest of the parameters of a map-creating request
//! (clause 5.5.14).
//!
//! Clause 5.5.14 holds every follow-up under a map to "the same parameters as
//! in the original request that created the EntityMap, except for the
//! specification of Entity identifiers or parameters related to pagination,
//! or, in the case of temporal requests, the temporal query". The digest is
//! therefore taken over the identity of the compiled query with its
//! identifiers excepted: the expanded type selector, `attrs`, `q`, the
//! geo-query, `scopeQ` and `csf`, each in its normal form. The identifiers and
//! `idPattern` (both a specification of Entity identifiers, so a source
//! holding a map may be asked for one mapped id where its map was created for a
//! pattern), `limit`/`offset`/`count`, the temporal window, `lastN` and every
//! representation parameter are not part of it. Types are compared as the IRIs
//! they expand to, so a compact name that expands to another IRI under another
//! `@context` is another selector.
//!
//! Clauses 5.7.2.3 and 5.7.4.3 give a query one selector of entity types. The
//! GET binding carries it as the `type` parameter and the POST binding as the
//! `type` of each `entities[]` selector (clause 5.2.33), and the identity
//! records it as the set of its clause 4.17 alternatives, which is the same
//! whichever binding carried it.

use crate::{entity_map_invariant::EntityMapInvariant, error::EntityMapError};
use andromeda_ngsi_ld_model::entity_map_fingerprint::{MapRequestKind, RequestFingerprint};
use andromeda_ngsi_ld_query::{
    entity_selection::EntitySelection,
    filter::reading::filter_reading::FilterReading,
    query_identity::{IdentifierScope, QueryIdentity},
    type_selection::grammar::TypeExpr,
};
use iri_rs::IriBuf;
use sha2::{Digest, Sha256};

/// The request a map is created for, by the operation it is, with the compiled
/// form of what it selects.
#[derive(Debug, Clone, Copy)]
pub enum FingerprintedRequest<'a> {
    /// Query Entities, through either binding, or Create `EntityMap` for Query Entities.
    QueryEntities {
        /// The compiled selection of the query.
        selection: &'a EntitySelection,
        /// The reading of the expanded context source filter, when the query
        /// carries one.
        csf: Option<&'a FilterReading>,
    },
    /// Retrieve Entity.
    RetrieveEntity {
        /// The expanded clause 5.7.1.3 type selector, when the request names one.
        entity_type: Option<&'a TypeExpr>,
        /// The expanded attribute list.
        attrs: &'a [IriBuf],
    },
    /// Query Temporal Evolution of Entities, through either binding, or its map creation.
    QueryTemporal {
        /// The compiled selection of the query.
        selection: &'a EntitySelection,
        /// The reading of the expanded context source filter, when the query
        /// carries one.
        csf: Option<&'a FilterReading>,
    },
    /// Retrieve Temporal Evolution of an Entity.
    RetrieveTemporal {
        /// The expanded attribute list.
        attrs: &'a [IriBuf],
    },
}

/// The digest of a request's compiled parameters, tagged with its operation.
///
/// # Errors
///
/// An identity that cannot be written as bytes, which is an invariant
/// violation of a value the request boundary already validated.
pub fn fingerprint_of(request: FingerprintedRequest<'_>) -> Result<RequestFingerprint, EntityMapError> {
    let (kind, identity) = match request {
        FingerprintedRequest::QueryEntities { selection, csf } => (MapRequestKind::QueryEntities, QueryIdentity::of(selection, csf, IdentifierScope::Excepted)),
        FingerprintedRequest::QueryTemporal { selection, csf } => (MapRequestKind::QueryTemporal, QueryIdentity::of(selection, csf, IdentifierScope::Excepted)),
        FingerprintedRequest::RetrieveEntity { entity_type, attrs } => (MapRequestKind::RetrieveEntity, QueryIdentity::of_retrieval(entity_type, attrs)),
        FingerprintedRequest::RetrieveTemporal { attrs } => (MapRequestKind::RetrieveTemporal, QueryIdentity::of_retrieval(None, attrs)),
    };
    let record = identity
        .record()
        .map_err(|error| EntityMapError::from(EntityMapInvariant::QueryIdentityUnwritable(error)))?;
    Ok(RequestFingerprint::new(kind, Sha256::digest(record).into()))
}

#[cfg(test)]
mod tests {
    use crate::{
        error::EntityMapError,
        fingerprint::{FingerprintedRequest, fingerprint_of},
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::{
        entity_map_fingerprint::{MapRequestKind, RequestComparison, RequestFingerprint},
        query::body::Query,
    };
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        filter::{compiled_filter::CompiledFilter, expression::Expr, language_matching::LanguageMatching},
        geo::query::GeoQuery,
        namespace_expander::{EXAMPLE_NAMESPACE, NamespaceExpander},
        projection::attrs_selection::{AttrsAdmission, expand_attrs_admitted},
        scope::query::ScopeQuery,
        selector_compilation::compile_entity_selectors,
        type_selection::expansion::expand_type_expression,
        written_entity_selector::WrittenEntitySelector,
    };
    use andromeda_ngsi_ld_request::{
        entity::GetEntityParams,
        query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters},
    };
    use iri_rs::iri;

    /// An expander mapping every compact name to `https://other.example/`.
    const OTHER_NAMESPACE: NamespaceExpander = NamespaceExpander::uniform(iri!("https://other.example/"));

    /// The selection a query compiles to under `expander`: its selectors, `attrs`, `q` and `scopeQ`.
    async fn compiled(
        selectors: &[WrittenEntitySelector],
        attrs: Option<&[AttributeName]>,
        q: Option<&Expr>,
        scope_q: Option<&ScopeQuery>,
        expander: &NamespaceExpander,
    ) -> EntitySelection {
        EntitySelection::builder()
            .selectors(compile_entity_selectors(selectors, expander).await.expect("selectors compile"))
            .attrs(
                expand_attrs_admitted(attrs.unwrap_or_default(), AttrsAdmission::AttributeNamesOnly, expander)
                    .await
                    .expect("attrs expand"),
            )
            .q(q.map(|expr| CompiledFilter::new(expr.clone(), LanguageMatching::TagEquality)))
            .scope_q(scope_q.cloned())
            .build()
    }

    async fn entity_query_under(query: &str, expander: &NamespaceExpander) -> Result<RequestFingerprint, EntityMapError> {
        let params = serde_urlencoded::from_str::<EntityQueryParameters>(query).expect("a valid test fixture");
        let selection = compiled(
            &params.selectors(),
            params.attrs.as_deref(),
            params.q.as_ref(),
            params.scope_q.as_ref(),
            expander,
        )
        .await;
        fingerprint_of(FingerprintedRequest::QueryEntities {
            selection: &selection,
            csf: None,
        })
    }

    async fn entity_query(query: &str) -> Result<RequestFingerprint, EntityMapError> {
        entity_query_under(query, &EXAMPLE_NAMESPACE).await
    }

    async fn temporal_query(query: &str) -> Result<RequestFingerprint, EntityMapError> {
        let params = serde_urlencoded::from_str::<TemporalQueryParameters>(query).expect("a valid test fixture");
        let selection = compiled(
            &params.selectors(),
            params.attrs.as_deref(),
            params.q.as_ref(),
            params.scope_q.as_ref(),
            &EXAMPLE_NAMESPACE,
        )
        .await;
        fingerprint_of(FingerprintedRequest::QueryTemporal {
            selection: &selection,
            csf: None,
        })
    }

    async fn retrieve(query: &str) -> Result<RequestFingerprint, EntityMapError> {
        let params = serde_urlencoded::from_str::<GetEntityParams>(query).expect("a valid test fixture");
        let entity_type = match params.entity_type.as_ref() {
            Some(raw) => Some(expand_type_expression(raw, &EXAMPLE_NAMESPACE).await.expect("the type expands")),
            None => None,
        };
        let attrs = expand_attrs_admitted(
            params.attrs.as_deref().unwrap_or_default(),
            AttrsAdmission::AttributeNamesOnly,
            &EXAMPLE_NAMESPACE,
        )
        .await
        .expect("attrs expand");
        fingerprint_of(FingerprintedRequest::RetrieveEntity {
            entity_type: entity_type.as_ref(),
            attrs: &attrs,
        })
    }

    /// A POST Query Entities body selecting `entities`.
    async fn posted_query(entities: &str) -> Result<RequestFingerprint, EntityMapError> {
        let query = serde_json::from_str::<Query>(&format!(r#"{{"type":"Query","entities":{entities}}}"#)).expect("a valid test fixture");
        let params = EntityQueryParameters::try_from(query).expect("a valid test fixture");
        let selection = compiled(&params.selectors(), None, None, None, &EXAMPLE_NAMESPACE).await;
        fingerprint_of(FingerprintedRequest::QueryEntities {
            selection: &selection,
            csf: None,
        })
    }

    /// A POST Query Temporal Evolution of Entities body selecting `entities`.
    async fn posted_temporal_query(entities: &str) -> Result<RequestFingerprint, EntityMapError> {
        let query = serde_json::from_str::<Query>(&format!(
            r#"{{"type":"Query","entities":{entities},"temporalQ":{{"timerel":"after","timeAt":"2020-01-01T00:00:00Z"}}}}"#
        ))
        .expect("a valid test fixture");
        let params = TemporalQueryParameters::try_from(query).expect("a valid test fixture");
        let selection = compiled(&params.selectors(), None, None, None, &EXAMPLE_NAMESPACE).await;
        fingerprint_of(FingerprintedRequest::QueryTemporal {
            selection: &selection,
            csf: None,
        })
    }

    #[tokio::test]
    async fn parameter_order_and_list_order_do_not_change_the_fingerprint() -> Result<(), EntityMapError> {
        let first = entity_query("type=Vehicle&attrs=speed,brand&q=speed%3E10&scopeQ=/Madrid").await?;
        let second = entity_query("scopeQ=/Madrid&q=speed%3E10&attrs=brand,speed&type=Vehicle").await?;
        pretty_assertions::assert_eq!(first.compare(&second), RequestComparison::Same);
        Ok(())
    }

    /// Clause 4.9: the terms of a conjunction are a set, so their order is not
    /// part of the request a map was created for.
    #[tokio::test]
    async fn the_order_of_the_terms_of_q_does_not_change_the_fingerprint() -> Result<(), EntityMapError> {
        let first = entity_query("type=Vehicle&q=a%3D%3D1%3Bb%3D%3D2").await?;
        let second = entity_query("type=Vehicle&q=b%3D%3D2%3Ba%3D%3D1").await?;
        pretty_assertions::assert_eq!(first.compare(&second), RequestComparison::Same);
        Ok(())
    }

    #[tokio::test]
    async fn identifiers_pagination_and_the_map_parameters_are_excepted() -> Result<(), EntityMapError> {
        let created = entity_query("type=Vehicle&entityMap=true&entityMapLifetime=PT1H").await?;
        let follow_up = entity_query("type=Vehicle&id=urn:ngsi-ld:Vehicle:1,urn:ngsi-ld:Vehicle:2&limit=1&offset=3&count=true&options=keyValues").await?;
        pretty_assertions::assert_eq!(created.compare(&follow_up), RequestComparison::Same);
        Ok(())
    }

    /// Clause 5.5.14 excepts the specification of Entity identifiers, which
    /// `idPattern` is as much as `id`: a broker forwarding under a map a source
    /// created for a pattern names the mapped ids instead.
    #[tokio::test]
    async fn an_id_pattern_is_excepted_like_the_identifiers() -> Result<(), EntityMapError> {
        let created = entity_query("type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:.*&entityMap=true").await?;
        pretty_assertions::assert_eq!(
            created.compare(&entity_query("type=Vehicle&id=urn:ngsi-ld:Vehicle:1").await?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(
            created.compare(&entity_query("type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:1.*").await?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(
            created.compare(&entity_query("type=Building&id=urn:ngsi-ld:Vehicle:1").await?),
            RequestComparison::Changed
        );
        let temporal = temporal_query("type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:.*&timerel=before&timeAt=2030-01-01T00:00:00Z").await?;
        pretty_assertions::assert_eq!(
            temporal.compare(&temporal_query("type=Vehicle&id=urn:ngsi-ld:Vehicle:1&timerel=before&timeAt=2030-01-01T00:00:00Z").await?),
            RequestComparison::Same
        );
        Ok(())
    }

    #[tokio::test]
    async fn a_changed_filter_is_a_changed_request() -> Result<(), EntityMapError> {
        let created = entity_query("type=Vehicle").await?;
        pretty_assertions::assert_eq!(created.compare(&entity_query("type=Vehicle&q=speed%3E99").await?), RequestComparison::Changed);
        pretty_assertions::assert_eq!(created.compare(&entity_query("type=Building").await?), RequestComparison::Changed);
        pretty_assertions::assert_eq!(created.compare(&entity_query("type=Vehicle&attrs=speed").await?), RequestComparison::Changed);
        pretty_assertions::assert_eq!(created.compare(&entity_query("type=Vehicle&scopeQ=/Madrid").await?), RequestComparison::Changed);
        Ok(())
    }

    #[tokio::test]
    async fn a_changed_geo_query_is_a_changed_request() -> Result<(), EntityMapError> {
        let within = |coordinates: &str| GeoQuery {
            georel: GeoRel::Within,
            geometry: serde_json::from_str::<Geometry>(&format!(r#"{{"type":"Polygon","coordinates":{coordinates}}}"#)).expect("a polygon"),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("an attribute name"),
        };
        let fingerprint = |geo: Option<GeoQuery>| {
            let selection = EntitySelection::builder().geo(geo).build();
            fingerprint_of(FingerprintedRequest::QueryEntities {
                selection: &selection,
                csf: None,
            })
        };
        let created = fingerprint(Some(within("[[[0,0],[1,0],[1,1],[0,1],[0,0]]]")))?;
        pretty_assertions::assert_eq!(
            created.compare(&fingerprint(Some(within("[[[0,0],[1,0],[1,1],[0,1],[0,0]]]")))?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(
            created.compare(&fingerprint(Some(within("[[[0,0],[2,0],[2,2],[0,2],[0,0]]]")))?),
            RequestComparison::Changed
        );
        pretty_assertions::assert_eq!(created.compare(&fingerprint(None)?), RequestComparison::Changed);
        Ok(())
    }

    /// Clause 5.5.9.1 keeps one `@context` throughout a walk, and clause 5.5.14
    /// holds a follow-up to the parameters of the creating request: the same
    /// compact `type` that an `@context` expands to another IRI selects other
    /// entities.
    #[tokio::test]
    async fn the_same_compact_type_under_two_contexts_is_a_changed_request() -> Result<(), EntityMapError> {
        let created = entity_query("type=Vehicle").await?;
        pretty_assertions::assert_eq!(
            created.compare(&entity_query_under("type=Vehicle", &OTHER_NAMESPACE).await?),
            RequestComparison::Changed
        );
        pretty_assertions::assert_eq!(
            created.compare(&entity_query_under("type=Vehicle", &EXAMPLE_NAMESPACE).await?),
            RequestComparison::Same
        );
        Ok(())
    }

    #[tokio::test]
    async fn the_temporal_window_and_last_n_are_excepted_for_temporal_maps() -> Result<(), EntityMapError> {
        let created = temporal_query("type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z&lastN=2").await?;
        let follow_up = temporal_query("type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z&timeproperty=modifiedAt").await?;
        pretty_assertions::assert_eq!(created.kind(), MapRequestKind::QueryTemporal);
        pretty_assertions::assert_eq!(created.compare(&follow_up), RequestComparison::Same);
        pretty_assertions::assert_eq!(
            created.compare(&temporal_query("type=Vehicle&q=speed%3E1&timerel=before&timeAt=2030-01-01T00:00:00Z").await?),
            RequestComparison::Changed
        );
        Ok(())
    }

    #[tokio::test]
    async fn a_retrieval_under_a_query_map_is_a_different_operation() -> Result<(), EntityMapError> {
        let created = entity_query("type=Vehicle").await?;
        pretty_assertions::assert_eq!(created.compare(&retrieve("").await?), RequestComparison::DifferentOperation);
        pretty_assertions::assert_eq!(retrieve("attrs=speed").await?.compare(&retrieve("attrs=speed").await?), RequestComparison::Same);
        pretty_assertions::assert_eq!(
            retrieve("attrs=speed").await?.compare(&retrieve("attrs=brand").await?),
            RequestComparison::Changed
        );
        Ok(())
    }

    /// Clause 5.7.1.3: the type selector narrows which registrations match the
    /// target entity, so it is part of what a retrieval map was created for.
    #[tokio::test]
    async fn a_retrieval_type_selector_is_part_of_the_fingerprint() -> Result<(), EntityMapError> {
        pretty_assertions::assert_eq!(
            retrieve("type=Vehicle").await?.compare(&retrieve("type=Vehicle").await?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(retrieve("type=Vehicle").await?.compare(&retrieve("").await?), RequestComparison::Changed);
        pretty_assertions::assert_eq!(
            retrieve("type=Vehicle").await?.compare(&retrieve("type=Building").await?),
            RequestComparison::Changed
        );
        pretty_assertions::assert_eq!(
            retrieve("type=Vehicle|Building").await?.compare(&retrieve("type=Building,Vehicle").await?),
            RequestComparison::Same
        );
        Ok(())
    }

    #[tokio::test]
    async fn a_post_body_selector_fingerprints_by_type_without_identifiers_or_pattern() -> Result<(), EntityMapError> {
        let created = posted_query(r#"[{"type":"Vehicle","idPattern":"urn:ngsi-ld:Vehicle:.*"},{"type":"Building"}]"#).await?;
        pretty_assertions::assert_eq!(
            created.compare(&posted_query(r#"[{"type":"Building"},{"type":"Vehicle","id":"urn:ngsi-ld:Vehicle:2"}]"#).await?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(
            created.compare(
                &posted_query(r#"[{"type":"Building"},{"type":"Vehicle","id":"urn:ngsi-ld:Vehicle:1"},{"type":"Vehicle","id":"urn:ngsi-ld:Vehicle:2"}]"#)
                    .await?
            ),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(created.compare(&posted_query(r#"[{"type":"Vehicle"}]"#).await?), RequestComparison::Changed);
        Ok(())
    }

    /// Clause 5.7.2.3 gives Query Entities one selector of entity types, which
    /// the GET `type` parameter and the POST `entities[]` carry alike.
    #[tokio::test]
    async fn one_type_digests_alike_through_either_binding() -> Result<(), EntityMapError> {
        let through_get = entity_query("type=Vehicle").await?;
        pretty_assertions::assert_eq!(through_get.compare(&posted_query(r#"[{"type":"Vehicle"}]"#).await?), RequestComparison::Same);
        pretty_assertions::assert_eq!(
            posted_query(r#"[{"type":"Vehicle","id":"urn:ngsi-ld:Vehicle:1"}]"#)
                .await?
                .compare(&through_get),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(
            through_get.compare(&posted_query(r#"[{"type":"Building"}]"#).await?),
            RequestComparison::Changed
        );
        Ok(())
    }

    /// The operands of a clause 4.17 disjunction and the selectors of a POST
    /// body are both alternatives, written in any order and either spelling of
    /// the operator.
    #[tokio::test]
    async fn type_alternatives_digest_alike_through_either_binding() -> Result<(), EntityMapError> {
        let through_get = entity_query("type=Vehicle|Building").await?;
        pretty_assertions::assert_eq!(
            through_get.compare(&posted_query(r#"[{"type":"Vehicle"},{"type":"Building"}]"#).await?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(
            through_get.compare(&posted_query(r#"[{"type":"Building|Vehicle"}]"#).await?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(through_get.compare(&entity_query("type=Building,Vehicle").await?), RequestComparison::Same);
        pretty_assertions::assert_eq!(
            through_get.compare(&entity_query("type=Vehicle|Building|Vehicle").await?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(
            entity_query("type=(Home;Vehicle)|Motorhome")
                .await?
                .compare(&posted_query(r#"[{"type":"Motorhome"},{"type":"(Home;Vehicle)"}]"#).await?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(through_get.compare(&posted_query(r#"[{"type":"Vehicle"}]"#).await?), RequestComparison::Changed);
        pretty_assertions::assert_eq!(
            posted_query(r#"[{"type":"Vehicle"},{"type":"Building"}]"#)
                .await?
                .compare(&entity_query("type=Vehicle|Tower").await?),
            RequestComparison::Changed
        );
        Ok(())
    }

    /// Clause 5.7.4.3 gives Query Temporal Evolution of Entities the same one
    /// selector of entity types.
    #[tokio::test]
    async fn a_temporal_type_selection_digests_alike_through_either_binding() -> Result<(), EntityMapError> {
        let through_get = temporal_query("type=Vehicle|Building&timerel=before&timeAt=2030-01-01T00:00:00Z").await?;
        pretty_assertions::assert_eq!(
            through_get.compare(&posted_temporal_query(r#"[{"type":"Building"},{"type":"Vehicle"}]"#).await?),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(
            through_get.compare(&posted_temporal_query(r#"[{"type":"Vehicle"}]"#).await?),
            RequestComparison::Changed
        );
        Ok(())
    }
}
