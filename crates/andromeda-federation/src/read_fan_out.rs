//! The registration selection and fan-out shared by every forwarded read
//! (clauses 5.7.1.4, 5.7.2.4, 5.7.3.4, 5.7.4.4 and 6.3.17).
//!
//! A current-entity read and a temporal read select their registrations the
//! same way and send them the same kind of GET; they differ only in the
//! resource path and in how one answer is read back, which is what the callers
//! supply.

use crate::{
    FederationService,
    cache::Cacheability,
    document_rewording::DocumentVocabulary,
    entity_read::mode_admitted,
    error::FederationError,
    operation::ForwardingOperation,
    outcome::{ForwardGetOutcome, ForwardReadOutcome, MatchedSources},
    read_binding::{ReadIntent, read_binding_for},
    registration_selection::registration_scope_admits,
    request::{CsrModeFilter, EntityMapInUse, TriggeringRequest},
    source_read::SourceRead,
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::term_lookup::PreparedTermContext};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, context_source_properties::ContextSourceProperties, forwarding_mode::ForwardingMode};
use andromeda_ngsi_ld_query::{
    filter::{eval::evaluate, expression::Expr},
    queried_entity::linked_entities::NoLinkedEntities,
    scope::query::ScopeQuery,
    type_selection::grammar::TypeExprRaw,
};
use futures::{StreamExt, stream};
use http::StatusCode;
use serde_json::{Map, Value};
use std::cell::OnceCell;
use tracing::warn;
use url::Url;

/// What a forwarded read selects its registrations by.
pub(crate) struct ReadSelection<'a> {
    /// The clause 4.17 type selector, whose single type is matched expanded.
    pub(crate) entity_type: Option<&'a TypeExprRaw>,
    /// The entity identifiers; the first one drives the store's matching.
    pub(crate) entity_ids: Option<&'a [Identifier]>,
    /// A registration never contacted (clause 5.8.6 re-assembly).
    pub(crate) exclude_registration: Option<&'a Identifier>,
    /// The expanded context source filter, evaluated against each
    /// registration's Context Source Properties (clause 4.9).
    pub(crate) csf: Option<&'a Expr>,
    /// The client's scope query, used to avoid contacting registrations whose
    /// declared footprint cannot satisfy it.
    pub(crate) scope_q: Option<&'a ScopeQuery>,
    /// What the registration must be able to serve for this read (clause 4.20).
    pub(crate) serving: ReadServing,
    /// The modes this forwarding phase admits (clause 4.3.6.2).
    pub(crate) mode_filter: CsrModeFilter,
    /// The `EntityMap` the read runs under (clause 5.5.14).
    pub(crate) entity_map: EntityMapInUse<'a>,
}

/// What a registration must be able to serve to be contacted for a read.
///
/// A retrieval (clauses 5.7.1.4, 5.7.3.4) and a temporal query are each bound
/// to one fixed operation. An entity query (clause 5.7.2) is bound per
/// registration to whichever of its two bindings the registration advertises
/// (clauses 4.3.6.1, 6.23.1), so it admits any registration that has a binding
/// for the client's query.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ReadServing {
    /// The registration must advertise this exact operation, or an operation
    /// group containing it (Table 4.20-2).
    Operation(ForwardingOperation),
    /// The registration must advertise one of the two Query Entities bindings
    /// (clause 6.23.1, Table 4.20-1).
    EntityQuery(ReadIntent),
}

impl ReadServing {
    /// Whether `csr` can serve this read.
    fn admits(self, csr: &ContextSourceRegistration) -> bool {
        match self {
            ReadServing::Operation(operation) => operation.matches(csr),
            ReadServing::EntityQuery(intent) => read_binding_for(intent, csr).is_some(),
        }
    }
}

/// One forwarded read, resolved to the request it sends: a GET (queryEntity or
/// retrieveEntity) or a POST batch query (queryBatch), each carrying the URL its
/// construction resolved to or the error that stopped it (clause 4.3.6.1).
pub(crate) enum ForwardedRead {
    /// A GET to `/entities` or `/entities/{id}`.
    Get(Result<Url, FederationError>),
    /// A POST to `/entityOperations/query` with an NGSI-LD `Query` body.
    PostQuery {
        /// The `/entityOperations/query` URL, or the error that stopped it.
        url: Result<Url, FederationError>,
        /// The serialized `Query` body (clause 6.23).
        body: Vec<u8>,
    },
}

/// What every GET of one fan-out carries, whichever registration receives it.
///
/// These four are one thing — the triggering request as each context source
/// sees it — and none of them varies between the sources contacted: the
/// forwarded requests all resolve against the same JSON-LD context, extend the
/// same `Via` chain and pass through the same headers (clauses 6.3.18, 6.3.19),
/// are cacheable on the same terms (clause 6.3.17), and name the same
/// `EntityMap` (clause 5.5.14).
#[derive(Clone, Copy)]
pub(crate) struct ForwardedGetConditions<'a> {
    /// The JSON-LD context the triggering request resolved to.
    pub(crate) context: &'a ContextSpec,
    /// The request that triggered the forwarding.
    pub(crate) triggering: TriggeringRequest<'a>,
    /// Whether the answers may be served from, and written to, the cache.
    pub(crate) cacheability: Cacheability,
    /// The `EntityMap` the read runs under, which fixes the location each
    /// source is told.
    pub(crate) entity_map: EntityMapInUse<'a>,
    /// What kind of answer every source returns, which decides where its names
    /// sit when it is read back in the client's terms (clause 4.3.6.6).
    pub(crate) vocabulary: DocumentVocabulary,
}

/// What one contacted registration answered, before the caller folds it.
pub(crate) struct SourceAnswer<T> {
    /// The registration's forwarding mode.
    pub(crate) mode: ForwardingMode,
    /// The error status the source answered with, when it answered with one.
    pub(crate) error_status: Option<u16>,
    /// The entry the answer was read into; `None` for a 404, which
    /// contributes nothing (clause 6.3.17).
    pub(crate) entry: Option<T>,
}

impl FederationService {
    /// The registrations a forwarded read contacts: those a live `EntityMap`
    /// names, or those clause 5.12 matches, narrowed to the ones advertising
    /// the operation, passing the context source filter and admitted by the
    /// phase's mode filter.
    pub(crate) async fn select_read_registrations(
        &self,
        context: &ContextSpec,
        selection: ReadSelection<'_>,
    ) -> Result<Vec<ContextSourceRegistration>, FederationError> {
        // Registrations store expanded type IRIs, so the compact selector is
        // expanded before matching.
        //
        // The context is prepared at most once for the whole selection, and
        // only when the type selector or the context source filter needs it.
        let prepared_context = OnceCell::new();
        let expanded_type = match selection.entity_type.and_then(TypeExprRaw::as_single_type) {
            Some(name) => {
                let prepared = prepared_context.get_or_init(|| self.engine.prepare_term_context(context));
                Some(self.engine.expand_type(name.as_str(), prepared).await?)
            }
            None => None,
        };
        let first_id = selection.entity_ids.and_then(|ids| ids.first());
        let registrations = match selection.entity_map.referenced() {
            Some(map) => self.registrations_of_map(map).await?,
            None => self.csr_repo.find_matching(expanded_type.as_ref(), first_id).await?,
        };
        let mut selected = Vec::with_capacity(registrations.len());
        for registration in registrations {
            if !selection.serving.admits(&registration)
                || selection.exclude_registration.is_some_and(|excluded| excluded == &registration.id)
                || !mode_admitted(&registration, selection.mode_filter)
                || !registration_scope_admits(&registration, selection.scope_q)
            {
                continue;
            }
            if let Some(filter) = selection.csf {
                let prepared = prepared_context.get_or_init(|| self.engine.prepare_term_context(context));
                if !self.passes_source_filter(&registration, filter, prepared).await {
                    continue;
                }
            }
            selected.push(registration);
        }
        Ok(selected)
    }

    /// Whether a registration passes the context source filter.
    ///
    /// Clause 4.9: the filter's target is "the name of a Context Source
    /// Property member of the `CSourceRegistration`, see Table 5.2.9-1", so the
    /// filter reads the whole registration: its clause 5.2.9 members such as
    /// `mode` and the Context Source Properties it adds. The filter was
    /// expanded against the request context, so every member name is expanded
    /// the same way before the two are compared; the added properties are
    /// stored under expanded names already.
    async fn passes_source_filter(&self, registration: &ContextSourceRegistration, filter: &Expr, prepared_context: &PreparedTermContext) -> bool {
        let document = match serde_json::to_value(registration) {
            Ok(Value::Object(members)) => members,
            Ok(Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_)) => return false,
            Err(error) => {
                warn!(registration = %registration.id, %error, "a stored registration could not be written back for the context source filter");
                return false;
            }
        };
        let mut expanded = Map::with_capacity(document.len());
        for (name, value) in document {
            match self.engine.expand_term(&name, prepared_context).await {
                Ok(iri) => {
                    expanded.insert(iri.to_string(), value);
                }
                Err(error) => {
                    warn!(registration = %registration.id, member = %name, %error, "a registration member name could not be expanded for the context source filter");
                }
            }
        }
        // Context Source Properties carry no Relationship a hop could cross.
        evaluate(filter, ContextSourceProperties::new(&expanded), NoLinkedEntities)
    }

    /// Send each target the forwarded read it resolved to -- a GET or a POST
    /// batch query -- telling each the location of the map in use (clause
    /// 5.5.14), and read each answer with `read`. Both answer with the entities
    /// they carried, so the caller reads them the same way (clause 6.3.17).
    pub(crate) async fn fan_out_read<T>(
        &self,
        targets: Vec<(ContextSourceRegistration, ForwardedRead)>,
        read: impl Fn(&ContextSourceRegistration, ForwardGetOutcome) -> Option<T> + Sync,
        conditions: ForwardedGetConditions<'_>,
    ) -> Vec<SourceAnswer<T>> {
        let read = &read;
        let ForwardedGetConditions {
            context,
            triggering,
            cacheability,
            entity_map,
            vocabulary,
        } = conditions;
        stream::iter(targets)
            .map(|(registration, forwarded)| async move {
                let source = SourceRead::builder()
                    .registration(&registration)
                    .inbound(context)
                    .triggering(triggering)
                    .linked_map(entity_map.location_for(&registration.id))
                    .vocabulary(vocabulary)
                    .build();
                let outcome = match forwarded {
                    ForwardedRead::Get(url) => self.forward_get(source, url, cacheability).await,
                    ForwardedRead::PostQuery { url, body } => self.forward_post_query(source, url, &body).await,
                };
                let error_status = outcome.error_status();
                SourceAnswer {
                    mode: registration.access.mode.unwrap_or_default(),
                    error_status,
                    entry: read(&registration, outcome),
                }
            })
            .buffer_unordered(self.csr_concurrency)
            .collect()
            .await
    }
}

/// Append one fan-out target per forwarded request a registration's blocks
/// produce (clause 4.3.6.3). Each target carries the registration that reads its
/// answer back, so the registration is cloned for every request but the last,
/// which moves it; a registration with a single covering block -- the common
/// case -- is never cloned.
pub(crate) fn expand_registration_targets<Q>(registration: ContextSourceRegistration, requests: Vec<Q>, targets: &mut Vec<(ContextSourceRegistration, Q)>) {
    let mut requests = requests.into_iter();
    let Some(mut current) = requests.next() else {
        return;
    };
    loop {
        if let Some(next) = requests.next() {
            targets.push((registration.clone(), current)); // clone: one target per forwarded request in a multi-block fan-out
            current = next;
        } else {
            targets.push((registration, current));
            break;
        }
    }
}

/// Fold the answers of `matched` contacted registrations into the outcome of
/// the read.
pub(crate) fn read_outcome<T>(answers: Vec<SourceAnswer<T>>, matched: usize) -> ForwardReadOutcome<T> {
    let single_source_status = match answers.as_slice() {
        // Table 6.4.3.2-3: a registered source alone holding the data answers
        // 501 when it cannot serve the representation, and that answer is
        // the operation's. An inclusive source declining is a clause 6.3.17
        // warning instead, because the rest of the data is still served.
        [only] if matched == 1 && matches!(only.mode, ForwardingMode::Exclusive | ForwardingMode::Redirect) => {
            only.error_status.filter(|status| *status == StatusCode::NOT_IMPLEMENTED.as_u16())
        }
        [] | [_] | [_, _, ..] => None,
    };
    ForwardReadOutcome {
        results: answers.into_iter().filter_map(|answer| answer.entry).collect(),
        sources: MatchedSources {
            registrations: matched,
            single_source_status,
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        fixed_registration_store::offline_service,
        operation::ForwardingOperation,
        outcome::ForwardedEntities,
        read_binding::ReadIntent,
        read_fan_out::{ReadSelection, ReadServing, SourceAnswer, read_outcome},
        request::{CsrModeFilter, EntityMapInUse},
    };
    use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
    use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
    use andromeda_ngsi_ld_query::filter::{expansion::expand_q_expr, json_key_attributes::JsonKeyAttributes, parser};
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

    fn answer(mode: ForwardingMode, error_status: Option<u16>) -> SourceAnswer<ForwardedEntities> {
        SourceAnswer {
            mode,
            error_status,
            entry: None,
        }
    }

    /// Table 6.4.3.2-3: the 501 of the one exclusive source holding the data is
    /// the operation's answer; the same 501 from an inclusive source, or from
    /// one of several sources, is not.
    #[test]
    fn a_lone_proxied_source_declining_the_representation_surfaces_its_501() {
        let exclusive = read_outcome(vec![answer(ForwardingMode::Exclusive, Some(501))], 1);
        pretty_assertions::assert_eq!(exclusive.sources.single_source_status, Some(501));
        let redirect = read_outcome(vec![answer(ForwardingMode::Redirect, Some(501))], 1);
        pretty_assertions::assert_eq!(redirect.sources.single_source_status, Some(501));

        let inclusive = read_outcome(vec![answer(ForwardingMode::Inclusive, Some(501))], 1);
        pretty_assertions::assert_eq!(inclusive.sources.single_source_status, None);
        let forbidden = read_outcome(vec![answer(ForwardingMode::Exclusive, Some(403))], 1);
        pretty_assertions::assert_eq!(forbidden.sources.single_source_status, None);
        let several = read_outcome(vec![answer(ForwardingMode::Exclusive, Some(501)), answer(ForwardingMode::Inclusive, None)], 2);
        pretty_assertions::assert_eq!(several.sources.single_source_status, None);
        pretty_assertions::assert_eq!(several.sources.registrations, 2);
    }

    /// Clause 5.7.1.4: a retrieval is forwarded only to registrations that
    /// "match and support the retrieveEntity operation" -- `retrieveEntity`
    /// itself, a Table 4.20-2 group containing it, or no `operations` at all,
    /// which clause 5.2.9 makes `federationOps`. One advertising only a query
    /// operation is never contacted for it, while an entity query still
    /// reaches it (clause 6.23.1).
    #[test]
    fn a_retrieval_admits_only_registrations_supporting_retrieve_entity() {
        let retrieval = ReadServing::Operation(ForwardingOperation::RetrieveEntity);
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

        let query = ReadServing::EntityQuery(ReadIntent::Query);
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
        let context = ContextSpec::from_body(json!({"sourceGroup": "https://example.org/sourceGroup"}));
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
                    .select_read_registrations(
                        context,
                        ReadSelection {
                            entity_type: None,
                            entity_ids: None,
                            exclude_registration: None,
                            csf: Some(&filter),
                            scope_q: None,
                            serving: ReadServing::Operation(ForwardingOperation::QueryEntity),
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
}
