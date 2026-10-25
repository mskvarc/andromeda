//! HTTP handling for temporal delete (clause 5.6.16).

use crate::temporal::{
    ContextSpec,
    ContextUrl,
    ForwardProvisionRequest,
    ForwardedWriteResult,
    ForwardingOperation,
    HeaderMap,
    Identifier,
    IncomingVia,
    IntoResponse,
    LocalOnlyParams,
    LocalOutcome,
    Method,
    OriginalUri,
    ProblemResponse,
    ProvisionResource,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantState,
    TriggeringRequest,
    TypedPath,
    ValidatedQuery,
    local_temporal_outcome,
    provision_response,
};
use andromeda_http_extraction::context_target;
use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;

/// Whether this broker removes the Temporal Evolution it holds once the
/// delete is forwarded.
///
/// Clause 5.6.16.4 ends the operation at a matching exclusive or redirect
/// registration ("No further processing is required"), whether or not that
/// registration advertises the operation, so the local Temporal Evolution
/// stays. Only an operation matching no such registration goes on to remove
/// it. Delete Entity (clause 5.6.6.4) has no such sentence and removes the
/// local Entity in both cases.
#[derive(Debug, PartialEq, Eq)]
enum LocalDeletion {
    /// A proxied registration matched: the local Temporal Evolution stays.
    Forgone,
    /// No proxied registration matched: the local Temporal Evolution, if any,
    /// is removed.
    Performed,
}

impl LocalDeletion {
    /// The local deletion the registrations `forwarded` reached leave to this
    /// broker.
    fn after(forwarded: &[ForwardedWriteResult]) -> LocalDeletion {
        if forwarded
            .iter()
            .any(|result| matches!(result.mode, ForwardingMode::Exclusive | ForwardingMode::Redirect))
        {
            LocalDeletion::Forgone
        } else {
            LocalDeletion::Performed
        }
    }
}

pub async fn delete(
    // Clause 6.3.20: the parameters are read first, before the tenant is
    // resolved, so an undefined one is an `InvalidRequest` on any tenant.
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath(id): TypedPath<Identifier>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;

    if params.local.unwrap_or(false) {
        service.delete(&target, &id).await.with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let context: ContextSpec = context.into_inner();
    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::TemporalEntities,
                entity_id: &id,
                entity_type_selector: None,
                attribute_name: None,
                method: Method::DELETE,
                context: &context,
                operation: ForwardingOperation::DeleteTemporalEntity,
                path_suffix: None,
                query_string: None,
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
            None,
        )
        .await
        .with_instance(&uri)?;

    let local = match LocalDeletion::after(&forwarded) {
        LocalDeletion::Forgone => LocalOutcome::NothingRemained,
        LocalDeletion::Performed => local_temporal_outcome(service.delete(&target, &id).await, &forwarded),
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::temporal::delete::LocalDeletion;
    use andromeda_federation::{
        answer_body::AnswerBody,
        outcome::{ForwardedWriteOutcome, ForwardedWriteResult},
    };
    use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
    use pretty_assertions::assert_eq;

    fn forwarded(mode: ForwardingMode, outcome: ForwardedWriteOutcome) -> ForwardedWriteResult {
        ForwardedWriteResult {
            registration_id: "urn:ngsi-ld:ContextSourceRegistration:test".parse().expect("valid URI"),
            mode,
            outcome,
            attributes: Vec::new(),
        }
    }

    fn deleted(mode: ForwardingMode) -> ForwardedWriteResult {
        forwarded(
            mode,
            ForwardedWriteOutcome::Completed {
                status: 204,
                body: AnswerBody::Absent,
            },
        )
    }

    #[test]
    fn an_exclusive_or_redirect_registration_leaves_the_local_temporal_evolution_in_place() {
        for mode in [ForwardingMode::Exclusive, ForwardingMode::Redirect] {
            assert_eq!(LocalDeletion::after(&[deleted(mode)]), LocalDeletion::Forgone, "{mode}");
            assert_eq!(
                LocalDeletion::after(&[deleted(ForwardingMode::Inclusive), deleted(mode)]),
                LocalDeletion::Forgone,
                "{mode}"
            );
        }
    }

    /// Clause 5.6.16.4 ends the operation at the matching registration whether
    /// or not it advertises the operation; its refusal is the Conflict.
    #[test]
    fn a_proxied_registration_not_advertising_the_delete_still_leaves_the_local_temporal_evolution_in_place() {
        assert_eq!(
            LocalDeletion::after(&[forwarded(ForwardingMode::Exclusive, ForwardedWriteOutcome::NotAdvertised)]),
            LocalDeletion::Forgone
        );
    }

    #[test]
    fn inclusive_registrations_alone_leave_the_local_temporal_evolution_to_be_removed() {
        assert_eq!(LocalDeletion::after(&[deleted(ForwardingMode::Inclusive)]), LocalDeletion::Performed);
        assert_eq!(LocalDeletion::after(&[]), LocalDeletion::Performed);
    }
}
