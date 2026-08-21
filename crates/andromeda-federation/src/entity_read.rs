//! Federated current-entity query and retrieve operations (clauses 5.7.1, 5.7.2, 6.3.17).

use crate::{
    FederationService,
    cache::Cacheability,
    entity_result::parse_forwarded_entities,
    error::FederationError,
    forwarding_url::{build_forwarding_query_string, build_query_url, build_retrieve_url},
    outcome::{ForwardGetOutcome, ForwardedEntities},
    request::{CsrModeFilter, ForwardQueryRequest, ForwardRetrieveRequest, TriggeringRequest},
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::{
    entity_map::EntityMap,
    registration::{ContextSourceRegistration, ForwardingMode},
};
use andromeda_ngsi_ld_query::{filter::eval::evaluate as eval_q_expr, type_selection::grammar::TypeExprRaw};
use futures::{StreamExt, stream};
use serde_json::Value;
use tracing::warn;
use url::Url;

/// Whether a registration's mode is admitted by a forwarding phase.
pub(crate) fn mode_admitted(registration: &ContextSourceRegistration, mode_filter: CsrModeFilter) -> bool {
    match mode_filter {
        CsrModeFilter::All => true,
        CsrModeFilter::ExcludeAuxiliary => registration.access.mode != Some(ForwardingMode::Auxiliary),
        CsrModeFilter::AuxiliaryOnly => registration.access.mode == Some(ForwardingMode::Auxiliary),
    }
}

/// The one entry a request that already looped back onto this broker
/// contributes: nothing is forwarded, and clause 6.3.17 names the loop with a
/// 199 warning.
pub(crate) fn own_loop_warning() -> NgsiLdWarning {
    NgsiLdWarning::MiscellaneousWarning("registration loop detected: the request's Via chain already names this context broker".to_string())
}

/// How one GET answer is read into forwarded entities.
#[derive(Debug, Clone, Copy)]
enum EntityShape {
    /// A query answers with an array of entities; a single object is tolerated.
    Collection,
    /// A retrieve answers with one entity object.
    Single,
}

impl FederationService {
    /// Read a forwarded GET into the entities it carried, with the clause
    /// 6.3.17 warning for every abnormal outcome. A 404 contributes nothing.
    fn entities_of(registration: &ContextSourceRegistration, outcome: ForwardGetOutcome, shape: EntityShape) -> Option<ForwardedEntities> {
        let mode = registration.access.mode.unwrap_or_default();
        let registration_id = registration.id.clone(); // clone: the result names the registration it came from
        let with_warnings = |entities, warnings| {
            Some(ForwardedEntities {
                registration_id: Some(registration_id.clone()), // clone: same
                mode,
                entities,
                warnings,
            })
        };
        match (shape, outcome) {
            (EntityShape::Collection, ForwardGetOutcome::Success(Value::Array(items), cs_warnings)) => {
                let (entities, parse_warnings) = parse_forwarded_entities(&registration.id, items);
                let mut warnings = cs_warnings;
                warnings.extend(parse_warnings);
                with_warnings(entities, warnings)
            }
            (EntityShape::Collection | EntityShape::Single, ForwardGetOutcome::Success(object @ Value::Object(_), cs_warnings)) => {
                let (entities, parse_warnings) = parse_forwarded_entities(&registration.id, vec![object]);
                let mut warnings = cs_warnings;
                warnings.extend(parse_warnings);
                with_warnings(entities, warnings)
            }
            (EntityShape::Collection | EntityShape::Single, ForwardGetOutcome::Success(_other, _)) => {
                warn!("Context source {} returned an unexpected response shape", registration.id);
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
                warn!("Forwarding to context source {} failed: {}", registration.id, warning);
                with_warnings(Vec::new(), vec![warning])
            }
        }
    }

    /// Fan a GET out to every selected registration, telling each the linked
    /// map the referenced EntityMap recorded for it (clause 5.5.14).
    async fn fan_out_get(
        &self,
        registrations: Vec<ContextSourceRegistration>,
        url_of: impl Fn(&ContextSourceRegistration) -> Result<Url, FederationError> + Sync,
        context: &ContextSpec,
        triggering: TriggeringRequest<'_>,
        cacheability: Cacheability,
        shape: EntityShape,
        entity_map: Option<&EntityMap>,
    ) -> Vec<ForwardedEntities> {
        let url_of = &url_of;
        stream::iter(registrations)
            .map(|registration| async move {
                let linked_map = entity_map.and_then(|map| map.linked_map_for(&registration.id));
                let outcome = self
                    .forward_get(&registration, url_of(&registration), context, triggering, cacheability, linked_map)
                    .await;
                FederationService::entities_of(&registration, outcome, shape)
            })
            .buffer_unordered(self.csr_concurrency)
            .filter_map(|entry| async move { entry })
            .collect()
            .await
    }

    /// Forward an entity query to matching context sources.
    ///
    /// `entity_type_compact` is the compact entity type from the query (e.g., "Vehicle").
    /// It will be expanded using the context for CSR matching.
    /// `entity_ids` are optional entity IDs to filter by (for query-by-id forwarding).
    pub async fn forward_query(&self, request: ForwardQueryRequest<'_>) -> Result<Vec<ForwardedEntities>, FederationError> {
        let ForwardQueryRequest {
            entity_type_compact,
            entity_ids,
            context,
            operation,
            triggering,
            csf_expr,
            mode_filter,
            entity_map,
        } = request;

        if self.is_loop_detected(triggering.via) {
            return Ok(vec![ForwardedEntities {
                registration_id: None,
                mode: ForwardingMode::Inclusive,
                entities: Vec::new(),
                warnings: vec![own_loop_warning()],
            }]);
        }

        // Expand entity type for CSR matching (CSRs store expanded IRIs)
        let prepared_context = self.engine.prepare_term_context(context);
        let expanded_type = if let Some(t) = entity_type_compact.and_then(TypeExprRaw::as_single_type) {
            Some(self.engine.expand_type(t.as_str(), &prepared_context).await?)
        } else {
            None
        };

        // Clause 5.5.14: a referenced EntityMap fixes the sources; otherwise
        // clause 5.12 matching selects them.
        let first_id = entity_ids.and_then(|ids| ids.first());
        let csrs = match entity_map {
            Some(map) => self.registrations_of_map(map).await?,
            None => self.csr_repo.find_matching(expanded_type.as_ref(), first_id).await?,
        };
        let matching_csrs: Vec<_> = csrs
            .into_iter()
            .filter(|csr| operation.matches(csr))
            .filter(|csr| {
                // Filter CSRs by csf expression against their additional_properties (S6.4.3.2)
                match csf_expr {
                    Some(expr) => match &csr.additional_properties {
                        Some(props) => eval_q_expr(expr, props),
                        None => false,
                    },
                    None => true,
                }
            })
            // Per spec 4.3.6.2: auxiliary CSRs only supplement existing data
            .filter(|csr| mode_admitted(csr, mode_filter))
            .collect();

        if matching_csrs.is_empty() {
            return Ok(Vec::new());
        }

        // Build a minimal forwarding query string — only type and id params
        // Context sources return all attributes; attribute filtering is done locally after merge
        let forwarding_qs = build_forwarding_query_string(entity_type_compact, entity_ids);
        let results = self
            .fan_out_get(
                matching_csrs,
                |csr| build_query_url(&csr.access.endpoint, &forwarding_qs),
                context,
                triggering,
                Cacheability::Cacheable,
                EntityShape::Collection,
                entity_map,
            )
            .await;
        Ok(results)
    }

    /// Forward an entity retrieval to matching context sources.
    ///
    /// Returns entities collected from all matching context sources.
    pub async fn forward_retrieve(&self, request: ForwardRetrieveRequest<'_>) -> Result<Vec<ForwardedEntities>, FederationError> {
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
            return Ok(vec![ForwardedEntities {
                registration_id: None,
                mode: ForwardingMode::Inclusive,
                entities: Vec::new(),
                warnings: vec![own_loop_warning()],
            }]);
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
            .fan_out_get(
                matching_csrs,
                |csr| build_retrieve_url(&csr.access.endpoint, entity_id, query_string),
                context,
                triggering,
                Cacheability::Cacheable,
                EntityShape::Single,
                entity_map,
            )
            .await;
        Ok(results)
    }
}
