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
    entity_read::mode_admitted,
    error::FederationError,
    forwarded_query::ForwardedQuery,
    operation::ForwardingOperation,
    outcome::{ForwardGetOutcome, ForwardReadOutcome, MatchedSources},
    registration_selection::{information_matches_entity, registration_scope_admits},
    request::{CsrModeFilter, EntityMapInUse, TriggeringRequest},
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::term_lookup::PreparedTermContext};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    entity_info::EntityTypeSelection,
    registration::{ContextSourceRegistration, ForwardingMode},
};
use andromeda_ngsi_ld_query::{
    filter::{eval::evaluate as eval_q_expr, expression::Expr},
    scope::query::ScopeQuery,
    type_selection::grammar::TypeExprRaw,
};
use futures::{StreamExt, stream};
use http::StatusCode;
use serde_json::{Map, Value};
use tracing::warn;
use url::Url;

/// What a forwarded read selects its registrations by.
pub(crate) struct ReadSelection<'a> {
    /// The clause 4.17 type selector, whose single type is matched expanded.
    pub(crate) entity_type: Option<&'a TypeExprRaw>,
    /// The entity identifiers; the first one drives the store's matching.
    pub(crate) entity_ids: Option<&'a [Uri]>,
    /// A registration never contacted (clause 5.8.6 re-assembly).
    pub(crate) exclude_registration: Option<&'a Uri>,
    /// The expanded context source filter, evaluated against each
    /// registration's Context Source Properties (clause 4.9).
    pub(crate) csf: Option<&'a Expr>,
    /// The client's scope query, used to avoid contacting registrations whose
    /// declared footprint cannot satisfy it.
    pub(crate) scope_q: Option<&'a ScopeQuery>,
    /// The operation the registration must advertise (clause 4.20).
    pub(crate) operation: ForwardingOperation,
    /// The modes this forwarding phase admits (clause 4.3.6.2).
    pub(crate) mode_filter: CsrModeFilter,
    /// The `EntityMap` the read runs under (clause 5.5.14).
    pub(crate) entity_map: EntityMapInUse<'a>,
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
        let expanded_type = match selection.entity_type.and_then(TypeExprRaw::as_single_type) {
            Some(name) => {
                let prepared_context = self.engine.prepare_term_context(context);
                Some(self.engine.expand_type(name.as_str(), &prepared_context).await?)
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
            if !selection.operation.matches(&registration)
                || selection.exclude_registration.is_some_and(|excluded| excluded == &registration.id)
                || !mode_admitted(&registration, selection.mode_filter)
                || !registration_scope_admits(&registration, selection.scope_q)
            {
                continue;
            }
            if let Some(filter) = selection.csf {
                let prepared_context = self.engine.prepare_term_context(context);
                if !self.passes_source_filter(&registration, filter, &prepared_context).await {
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
        eval_q_expr(filter, &Value::Object(expanded))
    }

    /// Send one GET to every target, telling each the location of the map in
    /// use (clause 5.5.14), and read each answer with `read`.
    pub(crate) async fn fan_out_get<T>(
        &self,
        targets: Vec<(ContextSourceRegistration, ForwardedQuery)>,
        url_of: impl Fn(&ContextSourceRegistration, &ForwardedQuery) -> Result<Url, FederationError> + Sync,
        read: impl Fn(&ContextSourceRegistration, ForwardGetOutcome) -> Option<T> + Sync,
        conditions: ForwardedGetConditions<'_>,
    ) -> Vec<SourceAnswer<T>> {
        let url_of = &url_of;
        let read = &read;
        let ForwardedGetConditions {
            context,
            triggering,
            cacheability,
            entity_map,
        } = conditions;
        stream::iter(targets)
            .map(|(registration, query)| async move {
                let linked_map = entity_map.location_for(&registration.id);
                let outcome = self
                    .forward_get(&registration, url_of(&registration, &query), context, triggering, cacheability, linked_map)
                    .await;
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

/// Fold the answers of `matched` contacted registrations into the outcome of
/// the read.
pub(crate) fn read_outcome<T>(answers: Vec<SourceAnswer<T>>, matched: usize, registered_type: Option<EntityTypeName>) -> ForwardReadOutcome<T> {
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
            registered_type,
            single_source_status,
        },
    }
}

/// The type the matched registrations register `entity_id` under, when every
/// selector naming the entity agrees on exactly one type (clause 5.2.10).
pub(crate) fn registered_type_of(registrations: &[ContextSourceRegistration], entity_id: &Uri) -> Option<EntityTypeName> {
    let mut types = registrations
        .iter()
        .flat_map(|registration| registration.coverage.information.iter())
        .filter(|information| information_matches_entity(information, None, Some(entity_id)))
        .flat_map(|information| information.entities.iter().flatten())
        .filter(|selector| selector.covers_id(entity_id))
        .filter_map(|selector| match &selector.type_ {
            EntityTypeSelection::Single(name) => Some(name),
            EntityTypeSelection::Several(_) => None,
        })
        .collect::<Vec<_>>();
    types.sort();
    types.dedup();
    match types.as_slice() {
        [only] => Some((*only).clone()), // clone: the outcome owns the type it answers with
        [] | [_, _, ..] => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        outcome::ForwardedEntities,
        read_fan_out::{SourceAnswer, read_outcome, registered_type_of},
    };
    use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, ForwardingMode};
    use serde_json::{Value, json};

    fn registration(information: &Value) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": information,
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
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
        let exclusive = read_outcome(vec![answer(ForwardingMode::Exclusive, Some(501))], 1, None);
        pretty_assertions::assert_eq!(exclusive.sources.single_source_status, Some(501));
        let redirect = read_outcome(vec![answer(ForwardingMode::Redirect, Some(501))], 1, None);
        pretty_assertions::assert_eq!(redirect.sources.single_source_status, Some(501));

        let inclusive = read_outcome(vec![answer(ForwardingMode::Inclusive, Some(501))], 1, None);
        pretty_assertions::assert_eq!(inclusive.sources.single_source_status, None);
        let forbidden = read_outcome(vec![answer(ForwardingMode::Exclusive, Some(403))], 1, None);
        pretty_assertions::assert_eq!(forbidden.sources.single_source_status, None);
        let several = read_outcome(
            vec![answer(ForwardingMode::Exclusive, Some(501)), answer(ForwardingMode::Inclusive, None)],
            2,
            None,
        );
        pretty_assertions::assert_eq!(several.sources.single_source_status, None);
        pretty_assertions::assert_eq!(several.sources.registrations, 2);
    }

    #[test]
    fn the_registered_type_is_the_one_every_selector_naming_the_entity_agrees_on() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse().expect("valid URI");
        let by_id = registration(&json!([{"entities": [{"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"}]}]));
        pretty_assertions::assert_eq!(
            registered_type_of(&[by_id], &entity_id).map(|name| name.as_ref().to_string()),
            Some("Vehicle".to_string())
        );

        let other_id = registration(&json!([{"entities": [{"id": "urn:ngsi-ld:Vehicle:2", "type": "Car"}]}]));
        assert!(registered_type_of(&[other_id], &entity_id).is_none());

        let disagreeing = [
            registration(&json!([{"entities": [{"idPattern": "urn:ngsi-ld:Vehicle:.*", "type": "Vehicle"}]}])),
            registration(&json!([{"entities": [{"type": "Car"}]}])),
        ];
        assert!(registered_type_of(&disagreeing, &entity_id).is_none());
    }
}
