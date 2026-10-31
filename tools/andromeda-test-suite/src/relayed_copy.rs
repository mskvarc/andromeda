//! The subscription copy a broker places on a context source (clause 5.8.1.4) and the notifications the source sends
//! back for it (clause 5.8.6): a test stubs the source's answers to the copy, reads the copy's identifier from what the
//! source received, and posts notifications to the broker under that identifier as the source would.

use crate::{
    body_pattern::BodyPattern,
    broker::BrokerClient,
    config::SuiteConfig,
    context_source::ContextSource,
    forwarded_body,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
    wire::{content_type, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, Response, StatusCode};
use iri_rs::{Iri, iri};
use mediatype::MediaType;
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    model::{Context, Entity, EntityTypeName, Lossless, Output},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
    subscription::{Notification, NotificationId, SubscriptionId},
};
use std::result::Result as StdResult;
use wiremock::Request;

/// The prefix an entity type gets in a subscription copy: the broker expands it under the default vocabulary.
const DEFAULT_VOCABULARY: Iri<&str> = iri!("https://uri.etsi.org/ngsi-ld/default-context/");

/// Makes the source, beneath its root path `prefix`, answer a created copy with 201 and every request on a copy with
/// 204.
///
/// # Errors
///
/// Returns the failure to mount an answer.
pub async fn accept_copies(source: &ContextSource, prefix: &str) -> Result<()> {
    source
        .answer(
            Method::POST,
            &format!("/{prefix}/ngsi-ld/v1/subscriptions"),
            source_answer::status(StatusCode::CREATED),
        )
        .await?;
    source
        .answer_when(
            SourceRequest::any_matching(&format!("/{prefix}/ngsi-ld/v1/subscriptions/.*")),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    Ok(())
}

/// Makes the source, beneath its root path `prefix`, accept copies ([`accept_copies`]) and registers it with the broker
/// as an inclusive source advertising `federationOps` for the test's `Vehicle` type and identifiers.
///
/// # Errors
///
/// Returns the failure to mount an answer or to register the source.
pub async fn register_copy_source(broker: &BrokerClient, source: &ContextSource, namespace: &Namespace, prefix: &str) -> Result<()> {
    register_with(broker, source, namespace, prefix, 1, None).await
}

/// As [`register_copy_source`], the registration carrying the test's registration number `number`, so a test can
/// register several sources of the one type and identifier pattern.
///
/// # Errors
///
/// Returns the failure to mount an answer or to register the source.
pub async fn register_numbered_copy_source(broker: &BrokerClient, source: &ContextSource, namespace: &Namespace, prefix: &str, number: u32) -> Result<()> {
    register_with(broker, source, namespace, prefix, number, None).await
}

/// As [`register_copy_source`], the registration naming the `ngsildConformance` version `conformance` in its
/// `contextSourceInfo`, so the broker amends what it forwards to that version (clause 4.3.6.6).
///
/// # Errors
///
/// Returns the failure to mount an answer or to register the source.
pub async fn register_copy_source_amended(broker: &BrokerClient, source: &ContextSource, namespace: &Namespace, prefix: &str, conformance: &str) -> Result<()> {
    register_with(broker, source, namespace, prefix, 1, Some(conformance)).await
}

/// Registers the source for copies under registration number `number`, naming `conformance` in its `contextSourceInfo`
/// when there is one.
async fn register_with(
    broker: &BrokerClient,
    source: &ContextSource,
    namespace: &Namespace,
    prefix: &str,
    number: u32,
    conformance: Option<&str>,
) -> Result<()> {
    accept_copies(source, prefix).await?;
    let builder = CSourceRegistration::builder(source.endpoint(prefix)?);
    let builder = match conformance {
        Some(version) => builder.context_source_info(("ngsildConformance", version)),
        None => builder,
    };
    let registration = builder
        .id(namespace.registration_id(number)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(namespace.entity_type("Vehicle")?).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    Ok(())
}

/// The one copy for `entity_type` the source, beneath its root path `prefix`, received, waiting for it up to the
/// delivery deadline.
///
/// # Errors
///
/// Returns the failure when the source does not receive exactly one such copy.
pub async fn placed_copy(source: &ContextSource, prefix: &str, entity_type: &EntityTypeName) -> Result<Request> {
    let selects_type = BodyPattern::SelectsBelow("$.entities".parse()?, format!("$[?(@.type == '{DEFAULT_VOCABULARY}{entity_type}')]").parse()?);
    let request = SourceRequest::new(Method::POST, &format!("/{prefix}/ngsi-ld/v1/subscriptions")).body(selects_type);
    let mut placed = source.wait_for_matching(&request, 1).await?;
    verify_that!(placed, len(eq(1)))?;
    placed.pop().or_fail()
}

/// The identifier of the one copy for `entity_type` the source, beneath its root path `prefix`, received.
///
/// # Errors
///
/// Returns the failure when the source does not receive exactly one such copy, or the copy carries no `id`.
pub async fn placed_copy_id(source: &ContextSource, prefix: &str, entity_type: &EntityTypeName) -> Result<SubscriptionId> {
    let copy = forwarded_body::subscription(&placed_copy(source, prefix, entity_type).await?)?;
    copy.id().cloned().or_fail()
}

/// The text of the notification the source sends for the copy `copy_id`: number 1 of the test, notified at a fixed
/// instant, holding `data` in normalized representation and naming `context`.
///
/// # Errors
///
/// Returns the failure when the notification identifier or the instant is invalid.
pub fn notification_body(namespace: &Namespace, copy_id: &SubscriptionId, data: Vec<Entity<Output>>, context: Context) -> Result<String> {
    let notification = Notification::builder()
        .id(namespace.urn_text("Notification", 1).parse::<NotificationId>()?)
        .subscription_id(copy_id.clone())
        .notified_at("2026-10-06T12:00:00Z".parse()?)
        .data(data)
        .context(context)
        .build();
    Ok(notification.render(Lossless::Normalized).to_string())
}

/// The text of the notification the source sends for the copy `copy_id` with `data`, a JSON array written as given,
/// naming the core `@context`: for entities the typed model cannot write (the NGSI-LD Null as a bare value, a context
/// on each entity).
///
/// # Errors
///
/// Returns the failure when the test configuration is missing.
pub fn notification_text(namespace: &Namespace, copy_id: &SubscriptionId, data: &str) -> Result<String> {
    notification_text_numbered(namespace, 1, copy_id, data)
}

/// As [`notification_text`], the notification carrying the test's notification number `number`, for a test whose
/// source sends several.
///
/// # Errors
///
/// Returns the failure when the test configuration is missing.
pub fn notification_text_numbered(namespace: &Namespace, number: u32, copy_id: &SubscriptionId, data: &str) -> Result<String> {
    let core = SuiteConfig::get().map_err(Clone::clone)?.core_context();
    Ok(assemble(&namespace.urn_text("Notification", number), copy_id, data, core.as_str()))
}

/// The notification document with the members of Table 5.3.1-1 around `data`.
fn assemble(notification_id: &str, copy_id: &SubscriptionId, data: &str, context: &str) -> String {
    format!(
        r#"{{"id":"{notification_id}","type":"Notification","subscriptionId":"{copy_id}","notifiedAt":"2026-10-06T12:00:00Z","data":{data},"@context":"{context}"}}"#
    )
}

/// Posts `body` to the broker's notification endpoint under `media_type`, as a context source does, and answers with the
/// response as received.
///
/// # Errors
///
/// Returns the failure when the request cannot be built; the broker's refusal is the answer.
pub async fn relay(broker: &BrokerClient, media_type: &MediaType<'_>, body: &str) -> Result<StdResult<Response<Bytes>, ClientError<ReqwestError>>> {
    let request = content_type(request_to(broker, Method::POST, "notifications")?, media_type)?.body(Bytes::copy_from_slice(body.as_bytes()));
    Ok(broker.send_unchecked(request).await)
}

#[cfg(test)]
mod tests {
    use crate::{
        namespace::Namespace,
        relayed_copy::{assemble, notification_body, notification_text_numbered},
    };
    use googletest::prelude::*;
    use ngsi_ld::model::{Context, Entity, EntityTypeName, Output};

    #[gtest]
    fn a_notification_names_the_copy_and_holds_its_entities_in_normalized_representation() -> Result<()> {
        let namespace = Namespace::fresh();
        let vehicle: Entity<Output> = Entity::output_builder()
            .id(namespace.urn("Vehicle", 1)?)
            .entity_type("Vehicle".parse::<EntityTypeName>()?)
            .property("speed", 40)
            .build()?;
        let context: Context = Context::from("https://example.test/context.jsonld".parse::<iri_rs::IriBuf>()?);
        let body = notification_body(&namespace, &"urn:ngsi-ld:Subscription:copy".parse()?, vec![vehicle], context)?;
        verify_that!(
            body,
            all!(
                contains_substring(r#""type":"Notification""#),
                contains_substring(r#""subscriptionId":"urn:ngsi-ld:Subscription:copy""#),
                contains_substring(r#""notifiedAt":"2026-10-06T12:00:00Z""#),
                contains_substring(r#""speed":{"type":"Property","value":40}"#)
            )
        )
    }

    #[gtest]
    fn a_numbered_notification_text_carries_the_number_in_its_identifier() -> Result<()> {
        let namespace = Namespace::fresh();
        let text = notification_text_numbered(&namespace, 2, &"urn:ngsi-ld:Subscription:copy".parse()?, "[]")?;
        verify_that!(text, contains_substring(format!(r#""id":"urn:ngsi-ld:Notification:{namespace}-2""#)))
    }

    #[gtest]
    fn a_notification_text_wraps_the_data_as_written() -> Result<()> {
        let text = assemble(
            "urn:ngsi-ld:Notification:1",
            &"urn:ngsi-ld:Subscription:copy".parse()?,
            r#"[{"owner":"urn:ngsi-ld:null"}]"#,
            "https://example.test/c",
        );
        verify_that!(
            text,
            eq(concat!(
                r#"{"id":"urn:ngsi-ld:Notification:1","type":"Notification","subscriptionId":"urn:ngsi-ld:Subscription:copy","#,
                r#""notifiedAt":"2026-10-06T12:00:00Z","data":[{"owner":"urn:ngsi-ld:null"}],"@context":"https://example.test/c"}"#
            ))
        )
    }
}
