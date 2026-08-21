//! Federated temporal entity query and retrieve operations (clauses 5.7.3, 5.7.4, 6.3.17).

use crate::{
    FederationService,
    cache::Cacheability,
    entity_read::{mode_admitted, own_loop_warning},
    error::FederationError,
    forwarding_url::{build_temporal_query_url, build_temporal_retrieve_url},
    outcome::{ForwardGetOutcome, ForwardedTemporalEntities},
    request::{ForwardRetrieveRequest, ForwardTemporalQueryRequest, TriggeringRequest},
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::{
    entity_map::EntityMap,
    registration::{ContextSourceRegistration, ForwardingMode},
};
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use futures::{StreamExt, stream};
use serde_json::Value;
use tracing::warn;
use url::Url;

/// How one temporal GET answer is read.
#[derive(Debug, Clone, Copy)]
enum TemporalShape {
    /// A query answers with an array; a single object is tolerated.
    Collection,
    /// A retrieve answers with one object.
    Single,
}

impl FederationService {
    /// The entry a request that already looped back onto this broker contributes.
    fn own_loop_entry() -> ForwardedTemporalEntities {
        ForwardedTemporalEntities {
            registration_id: None,
            mode: ForwardingMode::Inclusive,
            entities: Vec::new(),
            warnings: vec![own_loop_warning()],
        }
    }

    /// Read a forwarded temporal GET, with the clause 6.3.17 warning for every
    /// abnormal outcome. A 404 contributes nothing.
    fn temporal_entities_of(registration: &ContextSourceRegistration, outcome: ForwardGetOutcome, shape: TemporalShape) -> Option<ForwardedTemporalEntities> {
        let mode = registration.access.mode.unwrap_or_default();
        let with_warnings = |entities, warnings| {
            Some(ForwardedTemporalEntities {
                registration_id: Some(registration.id.clone()), // clone: the result names the registration it came from
                mode,
                entities,
                warnings,
            })
        };
        match (shape, outcome) {
            (TemporalShape::Collection, ForwardGetOutcome::Success(Value::Array(items), cs_warnings)) => with_warnings(items, cs_warnings),
            (TemporalShape::Collection | TemporalShape::Single, ForwardGetOutcome::Success(object @ Value::Object(_), cs_warnings)) => {
                with_warnings(vec![object], cs_warnings)
            }
            (TemporalShape::Collection | TemporalShape::Single, ForwardGetOutcome::Success(_other, _)) => {
                warn!("Context source {} returned an unexpected temporal response shape", registration.id);
                with_warnings(
                    Vec::new(),
                    vec![NgsiLdWarning::RevalidationFailed(format!(
                        "Context source {} returned unexpected response type",
                        registration.id
                    ))],
                )
            }
            (_, ForwardGetOutcome::NotFound) => None,
            (_, other) => {
                let warning = other.warning(&registration.id)?;
                warn!("Temporal forwarding to context source {} failed: {}", registration.id, warning);
                with_warnings(Vec::new(), vec![warning])
            }
        }
    }

    /// Fan a temporal GET out to every selected registration, telling each the
    /// linked map the referenced EntityMap recorded for it (clause 5.5.14).
    async fn fan_out_temporal_get(
        &self,
        registrations: Vec<ContextSourceRegistration>,
        url_of: impl Fn(&ContextSourceRegistration) -> Result<Url, FederationError> + Sync,
        context: &ContextSpec,
        triggering: TriggeringRequest<'_>,
        shape: TemporalShape,
        entity_map: Option<&EntityMap>,
    ) -> Vec<ForwardedTemporalEntities> {
        let url_of = &url_of;
        stream::iter(registrations)
            .map(|registration| async move {
                let linked_map = entity_map.and_then(|map| map.linked_map_for(&registration.id));
                // Clause 6.3.17 allows caching for `/entities` only.
                let outcome = self
                    .forward_get(&registration, url_of(&registration), context, triggering, Cacheability::Uncacheable, linked_map)
                    .await;
                FederationService::temporal_entities_of(&registration, outcome, shape)
            })
            .buffer_unordered(self.csr_concurrency)
            .filter_map(|entry| async move { entry })
            .collect()
            .await
    }

    /// Forward a temporal entity query to matching registrations.
    pub async fn forward_temporal_query(&self, request: ForwardTemporalQueryRequest<'_>) -> Result<Vec<ForwardedTemporalEntities>, FederationError> {
        let ForwardTemporalQueryRequest {
            entity_type_compact,
            entity_ids,
            query_string,
            context,
            operation,
            triggering,
            mode_filter,
            entity_map,
        } = request;
        if self.is_loop_detected(triggering.via) {
            return Ok(vec![FederationService::own_loop_entry()]);
        }

        let prepared_context = self.engine.prepare_term_context(context);
        let expanded_type = if let Some(t) = entity_type_compact.and_then(TypeExprRaw::as_single_type) {
            Some(self.engine.expand_type(t.as_str(), &prepared_context).await?)
        } else {
            None
        };

        let first_id = entity_ids.and_then(|ids| ids.first());
        let csrs = match entity_map {
            Some(map) => self.registrations_of_map(map).await?,
            None => self.csr_repo.find_matching(expanded_type.as_ref(), first_id).await?,
        };
        let matching_csrs: Vec<_> = csrs
            .into_iter()
            .filter(|csr| operation.matches(csr))
            // Per spec 4.3.6.2: auxiliary CSRs only supplement existing data
            .filter(|csr| mode_admitted(csr, mode_filter))
            .collect();

        if matching_csrs.is_empty() {
            return Ok(Vec::new());
        }

        let results = self
            .fan_out_temporal_get(
                matching_csrs,
                |csr| build_temporal_query_url(&csr.access.endpoint, query_string),
                context,
                triggering,
                TemporalShape::Collection,
                entity_map,
            )
            .await;
        Ok(results)
    }

    /// Forward a temporal entity retrieval to matching context sources.
    ///
    /// Same pattern as `forward_retrieve()` but targets `/ngsi-ld/v1/temporal/entities/{id}`.
    pub async fn forward_temporal_retrieve(&self, request: ForwardRetrieveRequest<'_>) -> Result<Vec<ForwardedTemporalEntities>, FederationError> {
        let ForwardRetrieveRequest {
            entity_id,
            exclude_registration,
            query_string,
            context,
            operation,
            triggering,
            mode_filter,
            entity_map,
        } = request;
        if self.is_loop_detected(triggering.via) {
            return Ok(vec![FederationService::own_loop_entry()]);
        }

        let csrs = match entity_map {
            Some(map) => self.registrations_of_map(map).await?,
            None => self.csr_repo.find_matching(None, Some(entity_id)).await?,
        };
        let matching_csrs: Vec<_> = csrs
            .into_iter()
            .filter(|csr| operation.matches(csr))
            .filter(|csr| exclude_registration.is_none_or(|excluded| excluded != &csr.id))
            // Per spec 4.3.6.2: auxiliary CSRs only supplement existing data
            .filter(|csr| mode_admitted(csr, mode_filter))
            .collect();

        if matching_csrs.is_empty() {
            return Ok(Vec::new());
        }

        let results = self
            .fan_out_temporal_get(
                matching_csrs,
                |csr| build_temporal_retrieve_url(&csr.access.endpoint, entity_id, query_string),
                context,
                triggering,
                TemporalShape::Single,
                entity_map,
            )
            .await;
        Ok(results)
    }
}
