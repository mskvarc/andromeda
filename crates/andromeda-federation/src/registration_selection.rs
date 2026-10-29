//! Which registrations a forwarded operation contacts (clauses 4.3.6, 5.5.14
//! and 5.12).
//!
//! Every operation that forwards a selection of entities (the reads, `EntityMap`
//! creation and Purge Entities) selects its registrations the same way: the
//! ones a live `EntityMap` names or the ones clause 5.12 matches, held to what
//! they serve, to the forwarding phase's modes, and to the filters the request
//! places on registrations.

use crate::{
    FederationService,
    entity_read::mode_admitted,
    error::FederationError,
    operation::ForwardingOperation,
    read_binding::{ReadIntent, read_binding_for},
    request::{CsrModeFilter, EntityMapInUse},
    selector_expansion::ExpandedSelectors,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    entity_map::EntityMap,
    registration::{
        ContextSourceRegistration,
        selection::{RegistrationFilters, TemporalPartition},
    },
};
use andromeda_ngsi_ld_query::{filter::expression::Expr, geo::query::GeoQuery, scope::query::ScopeQuery};
use andromeda_ngsi_ld_request::query::entity_selector::EntitySelectorParams;
use chrono::Utc;
use tracing::warn;

/// What a forwarded operation selects its registrations by.
pub(crate) struct RegistrationSelection<'a> {
    /// The union of entity selectors the operation asks for (clause 5.2.33): a
    /// registration is contacted when some selector matches it (clause 5.12), each
    /// by its own type selector, identifiers and identifier pattern.
    pub(crate) selectors: &'a [EntitySelectorParams],
    /// A registration never contacted (clause 5.8.6 re-assembly).
    pub(crate) exclude_registration: Option<&'a Identifier>,
    /// The expanded context source filter, evaluated against each
    /// registration's Context Source Properties (clause 4.9).
    pub(crate) csf: Option<&'a Expr>,
    /// The client's scope query, used to avoid contacting registrations whose
    /// declared footprint cannot satisfy it.
    pub(crate) scope_q: Option<&'a ScopeQuery>,
    /// The client's geoquery, compiled: a registration whose declared geometry
    /// cannot hold a matching Entity is not contacted (clauses 4.3.6.1 and
    /// 5.10.2.4).
    pub(crate) geo: Option<&'a GeoQuery>,
    /// What the registration must be able to serve (clause 4.20).
    pub(crate) serving: RegistrationServing,
    /// The modes this forwarding phase admits (clause 4.3.6.2).
    pub(crate) mode_filter: CsrModeFilter,
    /// The `EntityMap` the operation runs under (clause 5.5.14).
    pub(crate) entity_map: EntityMapInUse<'a>,
}

/// What a registration must be able to serve to be selected.
///
/// A retrieval (clauses 5.7.1.4, 5.7.3.4), a temporal query and an `EntityMap`
/// creation (clauses 5.14.4.4, 5.14.5.4) are each bound to one fixed
/// operation. An entity query (clause 5.7.2) is bound per registration to
/// whichever of its two bindings the registration advertises (clauses 4.3.6.1,
/// 6.23.1), so it admits any registration that has a binding for the client's
/// query. A purge answers for a matching registration that cannot serve it
/// (clause 5.6.21.4), so it selects every one.
#[derive(Debug, Clone, Copy)]
pub(crate) enum RegistrationServing {
    /// The registration must advertise this exact operation, or an operation
    /// group containing it (Table 4.20-2).
    Operation(ForwardingOperation),
    /// The registration must advertise one of the two Query Entities bindings
    /// (clause 6.23.1, Table 4.20-1).
    EntityQuery(ReadIntent),
    /// Every matching registration, whatever it advertises: the caller decides
    /// what one that cannot serve the operation means.
    Unchecked,
}

impl RegistrationServing {
    /// Whether `csr` is selected by what it serves.
    fn admits(self, csr: &ContextSourceRegistration) -> bool {
        match self {
            RegistrationServing::Operation(operation) => operation.matches(csr),
            RegistrationServing::EntityQuery(intent) => read_binding_for(intent, csr).is_some(),
            RegistrationServing::Unchecked => true,
        }
    }
}

impl FederationService {
    /// The registrations a forwarded operation contacts: those a live
    /// `EntityMap` names, or those clause 5.12 matches, narrowed to the ones
    /// serving the operation, admitted by the phase's mode filter and passing
    /// the request's registration filters.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Query`] when a selector's type cannot be
    /// expanded against `context`, and [`FederationError::Registration`] when
    /// the registrations cannot be read.
    pub(crate) async fn select_registrations(
        &self,
        context: &ContextSpec,
        selection: RegistrationSelection<'_>,
    ) -> Result<Vec<ContextSourceRegistration>, FederationError> {
        // Registrations store expanded type IRIs, so each selector's compact
        // type is expanded before matching.
        let registrations = if let Some(map) = selection.entity_map.referenced() {
            self.registrations_of_map(map).await?
        } else {
            let prepared = self.engine.prepare_term_context(context);
            let expanded = ExpandedSelectors::expand(&self.engine, &prepared, selection.selectors).await?;
            self.csr_repo.find_matching(&expanded.specifications()).await?
        };
        // A forwarded operation matches by clause 5.12, which has no temporal
        // axis.
        let filters = RegistrationFilters {
            scope_q: selection.scope_q,
            geo: selection.geo,
            csf: selection.csf,
            temporal: TemporalPartition::NotApplied,
        };
        Ok(registrations
            .into_iter()
            .filter(|registration| {
                selection.serving.admits(registration)
                    && selection.exclude_registration.is_none_or(|excluded| excluded != &registration.id)
                    && mode_admitted(registration, selection.mode_filter)
                    && filters.admits(registration).unwrap_or_else(|error| {
                        warn!(registration = %registration.id, %error, "the Context Source Properties of a stored registration could not be read for the context source filter");
                        false
                    })
            })
            .collect())
    }

    /// The registrations a request that references a live `EntityMap` contacts:
    /// the map's sources, looked up by identifier, in place of clause 5.12
    /// matching (clause 5.5.14, clause 5.12 last paragraph).
    async fn registrations_of_map(&self, map: &EntityMap) -> Result<Vec<ContextSourceRegistration>, FederationError> {
        let mut registrations = Vec::new();
        for registration_id in map.registrations() {
            if let Some(registration) = self.csr_repo.get(registration_id).await?
                && registration.lifecycle.in_force_at(Utc::now())
            {
                registrations.push(registration);
            }
        }
        Ok(registrations)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        fixed_registration_store::offline_service,
        operation::ForwardingOperation,
        read_binding::ReadIntent,
        registration_selection::{RegistrationSelection, RegistrationServing},
        request::{CsrModeFilter, EntityMapInUse},
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
    use andromeda_ngsi_ld_lexicon::time::rendering::instant_text;
    use andromeda_ngsi_ld_model::{entity_map::EntityMap, registration::ContextSourceRegistration};
    use andromeda_ngsi_ld_query::{
        filter::{expansion::expand_q_expr, json_key_attributes::JsonKeyAttributes, parser},
        geo::query::GeoQuery,
        scope::query::ScopeQuery,
    };
    use chrono::{Duration, Utc};
    use serde_json::json;

    fn registration(operations: Option<&[&str]>) -> ContextSourceRegistration {
        let mut document = json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [{"entities": [{"type": "https://example.org/Vehicle"}]}],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let Some(operations) = operations {
            document["operations"] = json!(operations);
        }
        serde_json::from_value(document).expect("valid registration")
    }

    /// Clause 5.7.1.4: a retrieval is forwarded only to registrations that
    /// "match and support the retrieveEntity operation" -- `retrieveEntity`
    /// itself, a Table 4.20-2 group containing it, or no `operations` at all,
    /// which clause 5.2.9 makes `federationOps`. One advertising only a query
    /// operation is never contacted for it, while an entity query still
    /// reaches it (clause 6.23.1).
    #[test]
    fn a_retrieval_admits_only_registrations_supporting_retrieve_entity() {
        let retrieval = RegistrationServing::Operation(ForwardingOperation::RetrieveEntity);
        for supporting in [
            &["retrieveEntity"][..],
            &["federationOps"],
            &["associationOps"],
            &["retrieveOps"],
            &["redirectionOps"],
        ] {
            assert!(retrieval.admits(&registration(Some(supporting))), "{supporting:?} supports retrieveEntity");
        }
        assert!(retrieval.admits(&registration(None)), "an absent operations member is federationOps");
        for unsupporting in [&["queryEntity"][..], &["queryBatch"], &["queryEntity", "queryBatch"], &["updateOps"]] {
            assert!(
                !retrieval.admits(&registration(Some(unsupporting))),
                "{unsupporting:?} does not support retrieveEntity"
            );
        }

        let query = RegistrationServing::EntityQuery(ReadIntent::Query);
        assert!(query.admits(&registration(Some(&["queryEntity"]))));
        assert!(query.admits(&registration(Some(&["queryBatch"]))));
        assert!(!query.admits(&registration(Some(&["retrieveEntity"]))));
    }
    /// A registration carrying `properties` beside its clause 5.2.9 members.
    fn registration_with(id: &str, properties: serde_json::Value) -> ContextSourceRegistration {
        let mut document = json!({
            "id": id,
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "mode": "inclusive",
            "information": [{"entities": [{"type": "https://example.org/Vehicle"}]}],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let (Some(members), serde_json::Value::Object(added)) = (document.as_object_mut(), properties) {
            members.extend(added);
        }
        serde_json::from_value(document).expect("valid registration")
    }

    /// Clause 4.9: a context source filter reads the Context Source Properties
    /// a registration adds and its own clause 5.2.9 members, each by the name
    /// the filter expands to under the request context.
    #[tokio::test]
    async fn a_source_filter_selects_by_the_properties_a_registration_carries() {
        let alpha = registration_with(
            "urn:ngsi-ld:csr:alpha",
            json!({"https://example.org/sourceGroup": {"type": "Property", "value": "alpha"}}),
        );
        let beta = registration_with("urn:ngsi-ld:csr:beta", json!({"https://example.org/sourceGroup": "beta"}));
        let service = offline_service(vec![alpha, beta]);
        let context =
            ContextSpec::from_body(JsonLiteral::admit(&json!({"sourceGroup": "https://example.org/sourceGroup"}).to_string()).expect("an admissible literal"));
        let selected = |q: &'static str| {
            let service = &service;
            let context = &context;
            async move {
                let prepared = service.engine().prepare_term_context(context);
                let filter = expand_q_expr(
                    parser::parse(q).expect("the filter parses"),
                    &JsonKeyAttributes::default(),
                    &ContextualExpander::new(service.engine(), &prepared),
                )
                .await
                .expect("the filter expands");
                service
                    .select_registrations(
                        context,
                        RegistrationSelection {
                            selectors: &[],
                            exclude_registration: None,
                            csf: Some(&filter),
                            scope_q: None,
                            geo: None,
                            serving: RegistrationServing::Operation(ForwardingOperation::QueryEntity),
                            mode_filter: CsrModeFilter::All,
                            entity_map: EntityMapInUse::None,
                        },
                    )
                    .await
                    .expect("the selection runs")
                    .into_iter()
                    .map(|registration| registration.id.to_string())
                    .collect::<Vec<_>>()
            }
        };
        pretty_assertions::assert_eq!(selected("sourceGroup==\"alpha\"").await, vec!["urn:ngsi-ld:csr:alpha"]);
        pretty_assertions::assert_eq!(selected("sourceGroup==\"beta\"").await, vec!["urn:ngsi-ld:csr:beta"]);
        pretty_assertions::assert_eq!(selected("mode==\"inclusive\"").await.len(), 2);
        pretty_assertions::assert_eq!(selected("sourceGroup==\"gamma\"").await, Vec::<String>::new());
    }

    /// Clauses 4.3.6.1 and 5.10.2.4: a registration located where no Entity
    /// the geoquery matches can be is not contacted; one declaring no location
    /// may serve any area.
    #[tokio::test]
    async fn a_geoquery_selects_the_registrations_whose_location_may_hold_a_match() {
        let madrid = registration_with("urn:ngsi-ld:csr:madrid", json!({"location": {"type": "Point", "coordinates": [-3.7, 40.4]}}));
        let berlin = registration_with("urn:ngsi-ld:csr:berlin", json!({"location": {"type": "Point", "coordinates": [13.4, 52.5]}}));
        let anywhere = registration_with("urn:ngsi-ld:csr:anywhere", json!({}));
        let service = offline_service(vec![madrid, berlin, anywhere]);
        let near_madrid = GeoQuery {
            georel: "near;maxDistance==100000".parse().expect("a valid georel"),
            geometry: serde_json::from_value(json!({"type": "Point", "coordinates": [-3.7, 40.4]})).expect("a valid point"),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("a valid attribute name"),
        };
        let selected: Vec<String> = service
            .select_registrations(
                &ContextSpec::none(),
                RegistrationSelection {
                    selectors: &[],
                    exclude_registration: None,
                    csf: None,
                    scope_q: None,
                    geo: Some(&near_madrid),
                    serving: RegistrationServing::Operation(ForwardingOperation::QueryEntity),
                    mode_filter: CsrModeFilter::All,
                    entity_map: EntityMapInUse::None,
                },
            )
            .await
            .expect("the selection runs")
            .into_iter()
            .map(|registration| registration.id.to_string())
            .collect();
        pretty_assertions::assert_eq!(selected, vec!["urn:ngsi-ld:csr:madrid", "urn:ngsi-ld:csr:anywhere"]);
    }

    /// A query selection naming nothing beyond the operation.
    fn unfiltered<'a>() -> RegistrationSelection<'a> {
        RegistrationSelection {
            selectors: &[],
            exclude_registration: None,
            csf: None,
            scope_q: None,
            geo: None,
            serving: RegistrationServing::Operation(ForwardingOperation::QueryEntity),
            mode_filter: CsrModeFilter::All,
            entity_map: EntityMapInUse::None,
        }
    }

    fn identifiers(registrations: Vec<ContextSourceRegistration>) -> Vec<String> {
        registrations.into_iter().map(|registration| registration.id.to_string()).collect()
    }

    /// Clauses 4.19 and 5.10.2.4: the Scope query is matched against the
    /// registration's `scope`, so a registration declaring none is not
    /// contacted for a scoped read.
    #[tokio::test]
    async fn a_scope_query_does_not_select_an_unscoped_registration() {
        let madrid = registration_with("urn:ngsi-ld:csr:madrid", json!({"scope": ["/Madrid"]}));
        let berlin = registration_with("urn:ngsi-ld:csr:berlin", json!({"scope": ["/Berlin"]}));
        let unscoped = registration_with("urn:ngsi-ld:csr:unscoped", json!({}));
        let service = offline_service(vec![madrid, berlin, unscoped]);
        let scope_q = ScopeQuery::parse("/Madrid").expect("a valid scope query");
        let selected = service
            .select_registrations(
                &ContextSpec::none(),
                RegistrationSelection {
                    scope_q: Some(&scope_q),
                    ..unfiltered()
                },
            )
            .await
            .expect("the selection runs");
        pretty_assertions::assert_eq!(identifiers(selected), vec!["urn:ngsi-ld:csr:madrid"]);
    }

    /// Table 5.2.9-1: a registration whose `expiresAt` has passed is no
    /// longer valid, so an `EntityMap` naming it does not bring it back
    /// (clause 5.12 last paragraph).
    #[tokio::test]
    async fn an_expired_registration_named_by_an_entity_map_is_not_contacted() {
        let expired = registration_with("urn:ngsi-ld:csr:expired", json!({"expiresAt": instant_text(Utc::now() - Duration::hours(1))}));
        let live = registration_with("urn:ngsi-ld:csr:live", json!({"expiresAt": instant_text(Utc::now() + Duration::hours(1))}));
        let service = offline_service(vec![expired, live]);
        let mut map = EntityMap::local(
            "urn:ngsi-ld:EntityMap:1".parse().expect("a valid identifier"),
            Utc::now() + Duration::hours(1),
            [],
        );
        for registration in ["urn:ngsi-ld:csr:expired", "urn:ngsi-ld:csr:live"] {
            map.add_remote_entities(
                &registration.parse().expect("a valid identifier"),
                ["urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier")],
            );
        }
        let selected = service
            .select_registrations(
                &ContextSpec::none(),
                RegistrationSelection {
                    entity_map: EntityMapInUse::Referenced(&map),
                    ..unfiltered()
                },
            )
            .await
            .expect("the selection runs");
        pretty_assertions::assert_eq!(identifiers(selected), vec!["urn:ngsi-ld:csr:live"]);
    }

    /// Clause 5.6.21.4: a purge answers for a matching registration that does
    /// not advertise `purgeEntity`, so its selection keeps one advertising no
    /// write at all.
    #[test]
    fn an_unchecked_selection_keeps_a_registration_whatever_it_advertises() {
        assert!(RegistrationServing::Unchecked.admits(&registration(Some(&["retrieveEntity"]))));
        assert!(RegistrationServing::Unchecked.admits(&registration(None)));
    }
}
