//! Typed handlers for the NGSI-LD Temporal API.

use crate::{
    error::TemporalResultExt as ResultInstanceExt,
    state::{TemporalTenantState as TenantState, TemporalTenantStateOrCreate as TenantStateOrCreate},
};
use andromeda_failure_classification::temporal_failure::classify_temporal_error;
use andromeda_federation::{
    entity_result::{collect_proxied_attrs, collect_temporal_warnings},
    operation::{ForwardingOperation, ProvisionResource},
    outcome::ForwardedWriteResult,
    request::{CsrModeFilter, ForwardProvisionRequest, ForwardTemporalQueryRequest, TriggeringRequest},
    temporal_result::merge_temporal_retrieve_results,
};
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    context::ContextUrl,
    jsonld::JsonLd,
    protocol::NgsiLdProtocolHeaders,
    query::ValidatedQuery,
    request_envelope::NgsiLdRequestEnvelope,
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_response::{
    collection::NgsiLdCollectionResponse,
    problem_response::ProblemResponse,
    provision::{LocalOutcome, provision_response},
    single::NgsiLdResponse,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, instance_identity::InstanceIdentity, time_rel::TimeRel};
use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
use andromeda_ngsi_ld_request::{attribute_deletion::DeleteAttrParams, local::LocalOnlyParams, query::temporal::TemporalQueryParameters};
use andromeda_temporal::{
    error::TemporalError,
    port::mutator::TemporalDeleteScope as DeleteScope,
    service::CreateOrUpdateResult,
    temporal_not_found_reason::TemporalNotFoundReason,
};
use axum::{
    extract::OriginalUri,
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};

/// The local outcome of a temporal write on an entity registrations also
/// hold: clauses 5.6.12.4 to 5.6.16.4 raise `ResourceNotFound` for an unknown
/// entity only when it is held neither locally nor by a matching registration,
/// so an entity without a local Temporal Evolution beside a matching
/// registration is nothing left to do here. A missing Attribute or instance of
/// an entity held here stays a failed part.
fn local_temporal_outcome<T>(result: Result<T, TemporalError>, forwarded: &[ForwardedWriteResult]) -> LocalOutcome {
    match result {
        Err(TemporalError::NotFound(TemporalNotFoundReason::Entity { .. } | TemporalNotFoundReason::UntypedEvolution { .. })) if !forwarded.is_empty() => {
            LocalOutcome::NothingRemained
        }
        other => LocalOutcome::processed(other, classify_temporal_error),
    }
}

/// Temporal add attributes handling.
pub(crate) mod add_attributes;
/// How far the merged history of a federated temporal read is complete
/// (clause 6.3.10).
mod complete_history;
/// Handles `POST /ngsi-ld/v1/temporal/entities`.
///
/// Creates a new temporal entity or updates an existing one from the request payload.
/// Temporal create or update handling.
pub(crate) mod create_or_update;
/// Temporal delete handling.
pub(crate) mod delete;
/// Temporal delete attribute handling.
pub(crate) mod delete_attribute;
/// Temporal delete attribute instance handling.
pub(crate) mod delete_attribute_instance;
/// Temporal history delete-by-query handling (the delete-by-query extension).
pub(crate) mod delete_by_query;
/// Where the entities a temporal `EntityMap` attributes to this broker come from.
pub(crate) mod local_map_entities;
/// Temporal query handling.
pub(crate) mod query;
/// Temporal retrieve handling.
pub(crate) mod retrieve;
/// Aggregating at this broker the entity of a federated temporal retrieval.
mod retrieve_aggregation;
/// The distributed half of a temporal retrieval.
mod retrieve_forwarding;
/// Temporal update attribute instance handling.
pub(crate) mod update_attribute_instance;

#[cfg(test)]
mod tests {
    use crate::temporal::local_temporal_outcome;
    use andromeda_federation::{
        answer_body::AnswerBody,
        outcome::{ForwardedWriteOutcome, ForwardedWriteResult},
    };
    use andromeda_http_response::provision::LocalOutcome;
    use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
    use andromeda_temporal::{error::TemporalError, temporal_not_found_reason::TemporalNotFoundReason};

    fn inclusive_success() -> ForwardedWriteResult {
        ForwardedWriteResult {
            registration_id: "urn:ngsi-ld:ContextSourceRegistration:test".parse().expect("valid URI"),
            mode: ForwardingMode::Inclusive,
            outcome: ForwardedWriteOutcome::Completed {
                status: 204,
                body: AnswerBody::Absent,
            },
            attributes: Vec::new(),
        }
    }

    #[test]
    fn an_entity_without_a_local_temporal_evolution_beside_a_registration_leaves_nothing_local() {
        let missing: Result<(), TemporalError> = Err(TemporalError::NotFound(TemporalNotFoundReason::Entity {
            id: "urn:ngsi-ld:Vehicle:1".parse().expect("valid URI"),
        }));
        assert!(matches!(local_temporal_outcome(missing, &[inclusive_success()]), LocalOutcome::NothingRemained));
    }

    #[test]
    fn a_missing_attribute_of_a_local_temporal_evolution_is_a_failed_local_part() {
        let missing: Result<(), TemporalError> = Err(TemporalError::NotFound(TemporalNotFoundReason::AttributeOnEntity {
            attribute: "speed".parse().expect("valid name"),
            entity: "urn:ngsi-ld:Vehicle:1".parse().expect("valid URI"),
        }));
        assert!(matches!(
            local_temporal_outcome(missing, &[inclusive_success()]),
            LocalOutcome::Processed(Err(_))
        ));
    }
}
