//! HTTP handling for the create entity operation.

use crate::{
    entity::{
        Entity,
        EntityResultExt,
        EntityTenantStateOrCreate,
        EntityTypeValue,
        ForwardProvisionRequest,
        ForwardedWriteResult,
        ForwardingOperation,
        HttpUri,
        Identifier,
        IncomingVia,
        IntoResponse,
        JsonLd,
        LocalOnlyParams,
        LocalOutcome,
        OriginalUri,
        ProblemResponse,
        ProvisionResource,
        Response,
        StatusCode,
        ValidatedQuery,
        classify_entity_error,
        collect_proxied_attrs,
        provision_response,
    },
    written_payload_admission::admit_written_entity,
};
use andromeda_entity::write_kind::CreateEntity;
use andromeda_federation::{forwarded_content::ForwardedContent, request::TriggeringRequest};
use andromeda_http_extraction::{
    context_target,
    resource_location::{ResourceCollection, ResourceLocator},
};
use andromeda_jsonld::user_context_admission::admit_user_context;
use andromeda_ngsi_ld_entity::admission::written_families::WrittenFamilies;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use axum::http::{HeaderMap, Method};

/// Create Entity (clause 5.6.1): 201 with the entity's `Location` when every
/// part succeeded, 207 with the clause 5.2.16 result when only some did. A
/// tenant that does not exist is created.
pub(crate) async fn create(
    context_target::ProvisionableTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    locator: ResourceLocator,
    JsonLd { body, context }: JsonLd<Entity>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.4 judges the payload "according to the @context", so the
    // context is admitted before anything is read under it (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_instance(&uri)?;
    let entity = body.admitted(&context).await?;
    let service = &state.entities;
    // Clause 5.6.1.4 opens with the clause 5.5.4 validation and forwards only
    // afterwards, so the whole screening runs here: a representation this
    // broker refuses reaches no context source, and a create whose attributes
    // a proxied registration claims in full is still validated rather than
    // answered 201 unexamined. Half of it is decidable only on the compact
    // payload: after expansion a seal forged outside `ngsildproof` is
    // indistinguishable from a sub-attribute (clauses 4.5.2.2, 4.5.3.2 and
    // 4.5.3.3).
    let admitted = admit_written_entity::<CreateEntity>(service, &entity, &context, WrittenFamilies::Final)
        .await
        .with_entity_instance(&uri)?;
    let is_local = params.local.unwrap_or(false);

    if is_local {
        let id = service.create(&target, &entity.id, admitted).await.with_entity_instance(&uri)?;
        return Ok(located(&locator, StatusCode::CREATED.into_response(), &id));
    }

    let entity_id = entity.id.clone(); // clone: the forwarding request and local create both need the identifier
    let entity_type = match &entity.type_ {
        EntityTypeValue::Single(name) => Some(name),
        EntityTypeValue::Multiple(names) => names.first(),
    }
    .map(|name| TypeExprRaw::Type(name.clone())); // clone: the one-type selector owns the type it names

    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::Entities,
                entity_id: &entity_id,
                entity_type_selector: entity_type.as_ref(),
                attribute_name: None,
                method: Method::POST,
                context: &context,
                operation: ForwardingOperation::CreateEntity,
                path_suffix: None,
                query_string: None,
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
            Some(ForwardedContent::Entity(admitted.document())),
        )
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        let id = service.create(&target, &entity_id, admitted).await.with_entity_instance(&uri)?;
        return Ok(located(&locator, StatusCode::CREATED.into_response(), &id));
    }

    // Clause 5.6.1.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains creates the entity here.
    let local = match admitted.local_remainder(&collect_proxied_attrs(&forwarded)) {
        Some(remainder) => LocalOutcome::processed(service.create(&target, &entity_id, remainder).await, classify_entity_error),
        None => LocalOutcome::NothingRemained,
    };
    distributed_create_response(&locator, &entity_id, &forwarded, &local, &uri)
}

/// Answer a Create Entity some of whose parts went to context sources: 201
/// when every part succeeded, 207 when some did, each naming the created
/// entity in its `Location` (Table 6.4.3.1-1).
///
/// # Errors
///
/// The complete operation failed, as [`provision_response`].
fn distributed_create_response(
    locator: &ResourceLocator,
    entity_id: &Identifier,
    forwarded: &[ForwardedWriteResult],
    local: &LocalOutcome,
    instance: &HttpUri,
) -> Result<Response, ProblemResponse> {
    provision_response(entity_id, forwarded, local, StatusCode::CREATED.into_response(), instance).map(|response| located(locator, response, entity_id))
}

/// The response with a `Location` header holding the relative path of the
/// created entity.
fn located(locator: &ResourceLocator, response: Response, id: &Identifier) -> Response {
    locator.locate(response, ResourceCollection::Entities, id)
}

#[cfg(test)]
mod tests {
    use crate::entity::create::distributed_create_response;
    use andromeda_failure_classification::classified_report::ClassifiedReport;
    use andromeda_federation::{
        answer_body::AnswerBody,
        outcome::{ForwardedWriteOutcome, ForwardedWriteResult},
    };
    use andromeda_http_extraction::resource_location::ResourceLocator;
    use andromeda_http_response::provision::LocalOutcome;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use axum::{
        http::{StatusCode, Uri as HttpUri},
        response::{IntoResponse, Response},
    };

    const ENTITY: &str = "urn:ngsi-ld:Vehicle:1";

    fn entity_id() -> Identifier {
        ENTITY.parse().expect("valid URI")
    }

    fn instance() -> HttpUri {
        HttpUri::from_static("/ngsi-ld/v1/entities")
    }

    fn inclusive(status: u16) -> ForwardedWriteResult {
        ForwardedWriteResult {
            registration_id: "urn:ngsi-ld:ContextSourceRegistration:test".parse().expect("valid URI"),
            mode: ForwardingMode::Inclusive,
            outcome: if status < 400 {
                ForwardedWriteOutcome::Completed {
                    status,
                    body: AnswerBody::Absent,
                }
            } else {
                ForwardedWriteOutcome::RemoteHttpFailure {
                    status,
                    body: AnswerBody::Absent,
                }
            },
            attributes: Vec::new(),
        }
    }

    fn location(response: &Response) -> Option<&str> {
        response.headers().get("location").and_then(|value| value.to_str().ok())
    }

    /// Table 6.4.3.1-1: the 207 includes a `Location` header holding the
    /// relative path of the created entity, as the 201 does.
    #[test]
    fn a_partly_failed_distributed_create_names_the_created_entity_in_its_location() {
        let response = distributed_create_response(
            &ResourceLocator::default(),
            &entity_id(),
            &[inclusive(500)],
            &LocalOutcome::Processed(Ok(())),
            &instance(),
        )
        .expect("a partial success");
        pretty_assertions::assert_eq!(response.status(), StatusCode::MULTI_STATUS);
        pretty_assertions::assert_eq!(location(&response), Some("/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1"));
    }

    #[test]
    fn a_distributed_create_every_part_of_which_succeeded_names_the_created_entity_in_its_location() {
        let response = distributed_create_response(
            &ResourceLocator::default(),
            &entity_id(),
            &[inclusive(201)],
            &LocalOutcome::Processed(Ok(())),
            &instance(),
        )
        .expect("a success");
        pretty_assertions::assert_eq!(response.status(), StatusCode::CREATED);
        pretty_assertions::assert_eq!(location(&response), Some("/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1"));
    }

    /// Clause 6.3.17: a create whose single redirect source loops back onto
    /// the broker, and of which nothing remained here, is 508 Loop Detected.
    #[test]
    fn a_create_whose_lone_redirect_source_loops_back_is_loop_detected() {
        let looped = ForwardedWriteResult {
            registration_id: "urn:ngsi-ld:ContextSourceRegistration:test".parse().expect("valid URI"),
            mode: ForwardingMode::Redirect,
            outcome: ForwardedWriteOutcome::LoopDetected,
            attributes: vec!["https://example.org/speed".parse().expect("a valid attribute name")],
        };
        let answer = distributed_create_response(
            &ResourceLocator::default(),
            &entity_id(),
            &[looped],
            &LocalOutcome::NothingRemained,
            &instance(),
        );
        let Err(problem) = answer else {
            panic!("a loop is the operation's error");
        };
        pretty_assertions::assert_eq!(problem.into_response().status(), StatusCode::LOOP_DETECTED);
    }

    #[test]
    fn a_distributed_create_that_failed_everywhere_is_the_local_error() {
        let conflict = ClassifiedReport::public(NgsiLdErrorType::AlreadyExists, "Entity urn:ngsi-ld:Vehicle:1 already exists");
        let answer = distributed_create_response(
            &ResourceLocator::default(),
            &entity_id(),
            &[inclusive(500)],
            &LocalOutcome::Processed(Err(conflict)),
            &instance(),
        );
        assert!(answer.is_err(), "no part created the entity");
    }
}
