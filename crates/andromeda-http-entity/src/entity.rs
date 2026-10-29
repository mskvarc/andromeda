use crate::{
    error::{EntityResultExt, public_error},
    join::{JoinRequest, ResolvedJoin},
    request_projection::RequestProjection,
    state::{EntityTenantState, EntityTenantStateOrCreate},
};
use andromeda_entity::{error::EntityError, merge_options::MergeOptions, query::retrieval::RetrievalNarrowing};
use andromeda_entity_map::contribution::SourceContribution;
use andromeda_federation::{
    entity_result::{collect_proxied_attrs, collect_warnings, contributions_of},
    operation::{ForwardingOperation, ProvisionResource},
    outcome::ForwardedWriteResult,
    provision_purge::{ForwardPurgeRequest, ForwardedPurgeResult},
    request::{CsrModeFilter, ForwardProvisionRequest, ForwardQueryRequest},
    single_source_failure::single_source_failure,
};
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::{
    accept::AcceptFormat,
    context::ContextUrl,
    entity_query::EntityQuery,
    jsonld::JsonLd,
    protocol::NgsiLdProtocolHeaders,
    query::ValidatedQuery,
    request_envelope::NgsiLdRequestEnvelope,
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_response::{
    batch_projection::{batch_operation_result, forwarded_write_error},
    collection::NgsiLdCollectionResponse,
    entity_failure::classify_entity_error,
    forwarding_failure::classify_forwarding_error,
    linked_resolution_warning::with_linked_resolution_warnings,
    problem_response::ProblemResponse,
    provision::{LocalOutcome, multi_status, provision_response},
    single::NgsiLdResponse,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::operation::selection::{DatasetSelection, OverwritePolicy};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    entity::{Entity, EntityPatch},
    entity_type::EntityTypeValue,
};
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchOperationResult, error_type::NgsiLdErrorType};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, projection::purge::PurgeMode};
use andromeda_ngsi_ld_render::geometry_selection::GeometrySelection;
use andromeda_ngsi_ld_request::{
    attribute_deletion::EntityAttributeDeletionParams,
    entity::{AppendParams, AttributeWriteParams, GetEntityParams, MergeParams},
    entity_write::{EntityWriteParams, UpdateAttributesParams},
    local::LocalOnlyParams,
    options::OverwritePolicy as RequestOverwritePolicy,
};
use axum::{
    extract::OriginalUri,
    http::{StatusCode, Uri as HttpUri},
    response::{IntoResponse, Response},
};

/// The local outcome of an entity write that matching registrations also
/// received: clauses 5.6.2.4 to 5.6.6.4, 5.6.17.4 and 5.6.18.4 raise
/// `ResourceNotFound` for an unknown Entity only when no matching registration
/// applies, so an Entity this broker does not hold leaves nothing to do here.
/// A missing Attribute or instance of an Entity it does hold stays a failed
/// part (clause 5.6.4.4).
fn local_entity_outcome<T>(result: Result<T, EntityError>, forwarded: &[ForwardedWriteResult]) -> LocalOutcome {
    match result {
        Err(EntityError::NotFound { .. }) if !forwarded.is_empty() => LocalOutcome::NothingRemained,
        other => LocalOutcome::processed(other, classify_entity_error),
    }
}

/// Attribute collection append handling.
pub mod append_attributes;
/// Entity creation handling.
pub mod create;
/// Entity deletion handling.
pub mod delete;
/// Attribute deletion handling.
pub mod delete_attribute;
/// Delete-by-query handling.
pub mod delete_by_query;
/// Entity merge handling.
pub mod merge;
/// Requests missing a required entity identifier.
pub mod missing_identifier;
/// Partial attribute update handling.
pub mod partial_update_attribute;
/// The `EntityMap` a purge determines its entities from.
pub mod purge_entity_map;
/// Entity collection query handling.
pub mod query;
/// Admitting a decoded entity query before it runs.
pub mod query_admission;
/// Which HTTP binding carried a Query Entities request.
pub mod query_binding;
/// The forwarding phases of an entity query.
pub mod query_forwarding;
/// Where the page of an entity query is cut.
pub mod query_page;
/// Assembling the response an entity query answers with.
pub mod query_response;
/// The filters `splitEntities` decides after the merge.
pub mod query_split_filters;
/// Entity replacement handling.
pub mod replace;
/// Attribute replacement handling.
pub mod replace_attribute;
/// The `EntityMap` a distributed read runs under.
pub mod request_entity_map;
/// Single-entity retrieval handling.
pub mod retrieve;
/// The forwarding phases of a single-entity retrieval.
pub mod retrieve_forwarding;
/// The forwarding of an attribute write, withheld from a local one.
pub mod scoped_provision;
/// Attribute collection update handling.
pub mod update_attributes;
/// The clause 5.2.18 `UpdateResult` an attribute write answers with.
pub mod update_result;

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::entity::local_entity_outcome;
    use andromeda_entity::error::EntityError;
    use andromeda_federation::{
        answer_body::AnswerBody,
        outcome::{ForwardedWriteOutcome, ForwardedWriteResult},
    };
    use andromeda_http_response::provision::LocalOutcome;
    use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;

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
    fn an_entity_held_only_by_a_matching_registration_leaves_nothing_local() {
        let missing: Result<(), EntityError> = Err(EntityError::NotFound {
            identifier: "urn:ngsi-ld:Vehicle:1".parse().expect("valid URI"),
        });
        assert!(matches!(local_entity_outcome(missing, &[inclusive_success()]), LocalOutcome::NothingRemained));
    }

    #[test]
    fn a_missing_attribute_of_a_local_entity_is_a_failed_local_part() {
        let missing: Result<(), EntityError> = Err(EntityError::AttributeNotFound {
            name: "speed".parse().expect("valid name"),
        });
        assert!(matches!(local_entity_outcome(missing, &[inclusive_success()]), LocalOutcome::Processed(Err(_))));
    }

    #[test]
    fn an_unknown_entity_with_no_registration_is_a_failed_local_part() {
        let missing: Result<(), EntityError> = Err(EntityError::NotFound {
            identifier: "urn:ngsi-ld:Vehicle:1".parse().expect("valid URI"),
        });
        assert!(matches!(local_entity_outcome(missing, &[]), LocalOutcome::Processed(Err(_))));
    }
}
