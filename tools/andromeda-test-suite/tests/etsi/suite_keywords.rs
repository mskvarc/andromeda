//! The keywords of the ETSI suite that several cases call, written once on the client's typed operations.

use crate::suite_payloads::{
    BUILDING_LOCATED_STREET_TERMS,
    BUILDING_SIMPLE_TERMS,
    VEHICLE_SPEED_FUEL_TERMS,
    building_located_at_city,
    building_located_in_city,
    building_located_in_paris,
    building_located_street_geo,
    building_registration,
    building_registration_subscription,
    building_simple_attributes,
    building_street_located,
    building_two_instances,
    bus_speed_fuel_level,
    city,
    city_in_france_described,
    city_paris_in_country,
    city_paris_in_france,
    country_described,
    country_france_described,
    key_values_document,
    key_values_notification,
    offstreet_pattern,
    pisa_tower_sightseeing,
    point,
    vehicle_brand_parked_speed_gps,
    vehicle_brand_parked_speed_sources,
    vehicle_fuel_level,
    vehicle_parked,
    vehicle_parked_speed_sources,
    vehicle_parking_name_fuel,
    vehicle_parking_registration,
    vehicle_speed_fuel_level,
    vehicle_speed_fuel_level_on,
    vehicle_speed_fuel_level_with_dataset,
    vehicle_twenty_instances,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    comparison::matches_document,
    config::SuiteConfig,
    context_source::ContextSource,
    eventually::{Poll, delivery_deadline, eventually},
    mqtt_observer::{MqttObserver, ReceivedMessage},
    namespace::Namespace,
    ngsild_date::is_ngsild_date,
    problem::{answered, problem},
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::{content_type, header_value, header_values, json_body, request_to, send_to},
};
use bytes::Bytes;
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::{
    HeaderMap,
    HeaderName,
    HeaderValue,
    Method,
    Response,
    StatusCode,
    Uri,
    header::{ACCEPT, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, LINK, LOCATION},
};
use iri_rs::{IriBuf, IriRefBuf};
use ngsi_ld::{
    client::{ClientError, Received, ReqwestError},
    http::{
        BatchCreateOutcome,
        BatchOperationResult,
        BatchOutcome,
        ContextDelivery,
        TemporalUpsertOutcome,
        UncheckedDocument,
        UncheckedRequest,
        UpsertOutcome,
        headers::{Accept, Link, LinkValue, RelationType},
        operation::{
            CreateEntity,
            CreateOrUpdateTemporalEvolution,
            CreateSubscription,
            Operation,
            Overwrite,
            PartialAttributeUpdate,
            RegisterContextSource,
            RepresentationSpelling,
            SystemAttributes,
            UpdateAttributes,
            UpdateRegistration,
            UpdateSubscription,
        },
    },
    model::{
        Attribute,
        AttributeKind,
        AttributeName,
        Change,
        Complete,
        Entity,
        EntityFragment,
        EntityId,
        ErrorType,
        Input,
        InstanceDeletion,
        InstanceId,
        Lossless,
        NgsiLdMediaType,
        Patch,
        ProblemDetails,
        Property,
        RawJson,
        Representation,
        evolution::TemporalEntity,
    },
    query::{EntitySelector, geoquery, temporal_query::TemporalProperty},
    registration::{CSourceRegistration, RegistrationFragment, RegistrationId, RegistrationInfo},
    subscription::{Activation, Endpoint, NotificationParams, RegistrationSubscription, Subscription, SubscriptionFragment, SubscriptionId},
};
use rumqttc::v5::mqttbytes::QoS;
use serde_json::{Value, json};
use std::{
    fmt::{Debug, Display},
    future::Future,
    time::Duration,
};
use tokio::time::sleep;

/// The `observedAt` the ETSI cases match for a whole second in UTC: `2020-09-09T16:40:00Z`, with at most six
/// fractional zeros.
pub(crate) const OBSERVED_AT_WHOLE_SECOND: &str = r"^2020-09-09T16:40:00(Z$|[.,](0{1,5}|0{6}\d*)Z)";

/// The media type of the JSON-LD bodies and `Content-Type` fields the ETSI keywords send.
pub(crate) const LD_JSON: &str = "application/ld+json";

/// The `observedAt` text of the member `name` of `body`, when it is a string.
pub(crate) fn observed_at<'a>(body: &'a Value, name: &str) -> Option<&'a str> {
    body.get(name)?.get("observedAt")?.as_str()
}

/// `Create Entity Selecting Content Type` with `application/ld+json`: posts `entity` to `/entities/` with the
/// vocabulary as a one-element `@context` list in the body, and requires 201.
pub(crate) async fn create_entity(broker: &BrokerClient, vocabulary: &Vocabulary, entity: &Entity<Input>) -> Result<()> {
    create_entity_answer(broker, vocabulary, entity).await.map(|_| ())
}

/// The client of `broker` that sends the `@context` of `vocabulary` in every request body, as `application/ld+json`.
pub(crate) fn ld_json_body_client(broker: &BrokerClient, vocabulary: &Vocabulary) -> Result<BrokerClient> {
    Ok(broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody))
}

/// Posts `entity` to `/entities/` with `client`, answering the broker's response whatever its status.
pub(crate) async fn send_create_entity(client: &BrokerClient, entity: &Entity<Input>) -> Result<Response<Bytes>, ClientError<ReqwestError>> {
    send_to(client, &CreateEntity::builder().entity(entity).build(), "entities/").await
}

/// `Create Entity Selecting Content Type` as [`create_entity`], answering the 201 response, whose head and body the
/// case reads.
pub(crate) async fn create_entity_answer(broker: &BrokerClient, vocabulary: &Vocabulary, entity: &Entity<Input>) -> Result<Response<Bytes>> {
    let client = ld_json_body_client(broker, vocabulary)?;
    let answer = send_create_entity(&client, entity).await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    Ok(answer?)
}

/// `Create Entity` with the vocabulary named in the `Link` header and no `@context` in the body: posts `entity` to
/// `/entities/` as `application/json`, and requires 201.
pub(crate) async fn create_entity_with_link(broker: &BrokerClient, vocabulary: &Vocabulary, entity: &Entity<Input>) -> Result<()> {
    let client = broker.with_context(vocabulary.context())?;
    let answer = send_to(&client, &CreateEntity::builder().entity(entity).build(), "entities/").await;
    verify_that!(&answer, answered(StatusCode::CREATED))
}

/// `Create Entity` whose body names `vocabulary` as a plain `@context` string rather than a list: posts `entity` to
/// `/entities/` as `application/ld+json`, and requires 201.
pub(crate) async fn create_entity_with_context_string(broker: &BrokerClient, vocabulary: &Vocabulary, entity: &Entity<Input>) -> Result<()> {
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let answer = send_create_entity(&client, entity).await;
    verify_that!(&answer, answered(StatusCode::CREATED))
}

/// `Create Entity` of `entity` as [`create_entity_with_context_string`] does, answering the response whatever its
/// status: the setup of a case that names `HTTP *`.
pub(crate) async fn create_entity_with_context_string_any(broker: &BrokerClient, vocabulary: &Vocabulary, entity: &Entity<Input>) -> Result<()> {
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    send_create_entity(&client, entity).await.any_answer();
    Ok(())
}

/// `Create Subscription` of `document`, the body `subscription` writes with the members the builder cannot spell, sent
/// with `client` and its `@context`: requires 201, and answers the cleanup that deletes the subscription.
pub(crate) async fn create_subscription_document(client: &BrokerClient, subscription: &Subscription<Input>, document: &UncheckedDocument) -> Result<Cleanup> {
    let cleanup = Cleanup::of(client).subscription_of(subscription);
    let request = client.unchecked(&CreateSubscription { subscription })?.document(document);
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    Ok(cleanup)
}

/// `Create Context Source Registration Subscription` of `subscription` with `document` as its body, sent by `client`
/// with the `@context` it carries: the request, unsent. The typed operation supplies the target and the headers.
pub(crate) fn registration_subscription_request(
    client: &BrokerClient,
    subscription: &Subscription<Input, RegistrationSubscription>,
    document: &UncheckedDocument,
) -> Result<UncheckedRequest> {
    Ok(client.unchecked(&CreateSubscription { subscription })?.document(document))
}

/// `Setup Initial Context Source Registration Subscription`: creates `subscription` with its notification to `uri`
/// written as the ETSI cases write it ([`key_values_document`]), sent by `client` with the `@context` it carries,
/// requires 201, and answers the cleanup that deletes the subscription.
pub(crate) async fn create_registration_subscription(
    client: &BrokerClient,
    subscription: &Subscription<Input, RegistrationSubscription>,
    uri: &url::Url,
) -> Result<Cleanup> {
    let cleanup = Cleanup::of(client).registration_subscription_of(subscription);
    let request = registration_subscription_request(client, subscription, &key_values_document(subscription, uri)?)?;
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    Ok(cleanup)
}

/// `Create Context Source Registration Subscription` of `subscription` as [`create_registration_subscription`] sends
/// it: requires 201, the empty body, and a `Location` naming the subscription's id; answers the cleanup that deletes
/// the subscription.
pub(crate) async fn expect_registration_subscription_created(
    client: &BrokerClient,
    subscription: &Subscription<Input, RegistrationSubscription>,
    uri: &url::Url,
) -> Result<Cleanup> {
    let cleanup = Cleanup::of(client).registration_subscription_of(subscription);
    let request = registration_subscription_request(client, subscription, &key_values_document(subscription, uri)?)?;
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    expect_created(&answer?, subscription.id().or_fail()?);
    Ok(cleanup)
}

/// `Setup Initial Context Source Registration Subscriptions` of the query cases, first part: registers the terms
/// `Building`, `name` and `subCategory` with `tenant`, then in that context the Building subscriptions
/// `urn:ngsi-ld:Subscription:<ns>-1` and `-2` (the second watching `name` and `subCategory`), both notifying `uri`
/// (201 each); answers the vocabulary, the ids and the cleanup that deletes both subscriptions.
pub(crate) async fn create_registration_subscription_pair(
    tenant: &BrokerClient,
    namespace: &Namespace,
    uri: &url::Url,
) -> Result<(Vocabulary, [SubscriptionId; 2], Cleanup)> {
    let vocabulary = Vocabulary::register(tenant, namespace, &["Building", "name", "subCategory"]).await?;
    let client = ld_json_body_client(tenant, &vocabulary)?;
    let first: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let second: SubscriptionId = namespace.urn_text("Subscription", 2).parse()?;
    let building = building_registration_subscription(Some(first.clone()), uri, Activation::Unspecified, None)?;
    let cleanup = create_registration_subscription(&client, &building, uri).await?;
    let watching = Subscription::registration_builder()
        .id(second.clone())
        .watched_attributes(["name", "subCategory"])
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(Endpoint::builder(uri.as_str()).build()?).build()?)
        .build()?;
    let cleanup = cleanup.absorbing(create_registration_subscription(&client, &watching, uri).await?);
    Ok((vocabulary, [first, second], cleanup))
}

/// `Setup Initial Context Source Registration Subscriptions` of the paging cases: the pair of
/// [`create_registration_subscription_pair`], then the Building subscription `urn:ngsi-ld:Subscription:<ns>-3` with
/// the `geoQ` near the point -8.52, 41.2 within 2000 m of `location`, notifying `uri` (201); answers the vocabulary
/// the three ids and the cleanup that deletes the three subscriptions.
pub(crate) async fn create_registration_subscription_triple(
    tenant: &BrokerClient,
    namespace: &Namespace,
    uri: &url::Url,
) -> Result<(Vocabulary, [SubscriptionId; 3], Cleanup)> {
    let (vocabulary, [first, second], cleanup) = create_registration_subscription_pair(tenant, namespace, uri).await?;
    let third: SubscriptionId = namespace.urn_text("Subscription", 3).parse()?;
    let near = Subscription::registration_builder()
        .id(third.clone())
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(Endpoint::builder(uri.as_str()).build()?).build()?)
        .geo_q(geoquery::near_max(2000, point(-8.52, 41.2)?).geoproperty("location").build()?)
        .build()?;
    let cleanup = cleanup.absorbing(create_registration_subscription(&ld_json_body_client(tenant, &vocabulary)?, &near, uri).await?);
    Ok((vocabulary, [first, second, third], cleanup))
}

/// Posts `registration` to `/csourceRegistrations` with `client`, answering the broker's response whatever its status.
pub(crate) async fn send_registration(client: &BrokerClient, registration: &CSourceRegistration<Input>) -> Result<Response<Bytes>, ClientError<ReqwestError>> {
    send_to(client, &RegisterContextSource { registration }, "csourceRegistrations").await
}

/// `Create Context Source Registration`: posts `registration` to `/csourceRegistrations` with the vocabulary as a
/// one-element `@context` list in the body, requires 201, and requires the `Location` of the answer to name the
/// registration's id; answers the cleanup that deletes the registration.
pub(crate) async fn create_registration(broker: &BrokerClient, vocabulary: &Vocabulary, registration: &CSourceRegistration<Input>) -> Result<Cleanup> {
    let cleanup = Cleanup::of(broker).registration_of(registration);
    let client = ld_json_body_client(broker, vocabulary)?;
    let answer = send_registration(&client, registration).await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    let id = registration.id().or_fail()?;
    expect_that!(header_value(answer?.headers(), &LOCATION), some(contains_substring(id.to_string())));
    Ok(cleanup)
}

/// The endpoint `sources/<ns>/<host>` of a fresh context source: the address under the mock origin the context
/// source cases register.
pub(crate) async fn csource_endpoint(namespace: &Namespace, host: &str) -> Result<url::Url> {
    Ok(ContextSource::start().await?.endpoint(&format!("sources/{namespace}/{host}"))?)
}

/// `Update Context Source Registration` of `id` with `document` as its body, sent by `client` in the context it
/// carries. The typed operation supplies the target and the headers; the empty fragment it names is replaced by
/// `document`.
pub(crate) fn update_registration_request(client: &BrokerClient, id: &RegistrationId, document: &UncheckedDocument) -> Result<UncheckedRequest> {
    let placeholder = RegistrationFragment::builder().build()?;
    Ok(client.unchecked(&UpdateRegistration { id, fragment: &placeholder })?.document(document))
}

/// `Setup Initial Context Source Registrations`: registers `terms` with `client`, then in that context the Vehicle
/// and parking registration `urn:ngsi-ld:ContextSourceRegistration:<ns>-1` and the [`building_registration`] `-2`,
/// both at the context source `sources/<ns>/my.csource.org:1026` (201 each); answers the vocabulary, the endpoint and
/// the cleanup that deletes both registrations.
pub(crate) async fn create_discovery_registrations(client: &BrokerClient, namespace: &Namespace, terms: &[&str]) -> Result<(Vocabulary, url::Url, Cleanup)> {
    let vocabulary = Vocabulary::register(client, namespace, terms).await?;
    let endpoint = csource_endpoint(namespace, "my.csource.org:1026").await?;
    let writer = ld_json_body_client(client, &vocabulary)?;
    let vehicle = vehicle_parking_registration(namespace, &endpoint, namespace.urn("Vehicle", "A456")?, None)?;
    let building = building_registration(namespace, 2, &endpoint)?;
    let cleanup = Cleanup::of(client).registration_of(&vehicle).registration_of(&building);
    writer.registrations().register(&vehicle).send().await?;
    writer.registrations().register(&building).send().await?;
    Ok((vocabulary, endpoint, cleanup))
}

/// `Query Context Source Registrations` written as the target `path`, with `vocabulary` in the `Link` header: requires
/// 400 `BadRequestData` and a title. The target is hand-written because the typed query refuses each invalid one.
pub(crate) async fn refused_registration_query(broker: &BrokerClient, vocabulary: &Vocabulary, path: &str) -> Result<()> {
    let request = request_to(broker, Method::GET, path)?.append_header(LINK, context_link(vocabulary)?);
    refused(broker, request, ErrorType::BadRequestData).await
}

/// The Retrieve Context Source Registration cases of the Vehicle and parking registration `-1`: registers `terms`
/// and the registration at the context source `sources/<ns>/my.csource.org:1026`, retrieves it with `Accept: */*`
/// (in the vocabulary's `Link` context when `context` is [`RetrievalContext::Link`]), requires 200 and the
/// registration with its terms compacted or expanded as `context` gives them, and deletes it (204).
pub(crate) async fn expect_vehicle_parking_retrieval(namespace: &Namespace, terms: &[&str], context: RetrievalContext) -> Result<()> {
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, namespace, terms).await?;
    let endpoint = csource_endpoint(namespace, "my.csource.org:1026").await?;
    let registration = vehicle_parking_registration(namespace, &endpoint, namespace.urn("Vehicle", "A456")?, None)?;
    let _registration = create_registration(&broker, &vocabulary, &registration).await?;
    let id = registration.id().or_fail()?;
    let reader = match context {
        RetrievalContext::Link => broker.with_context(vocabulary.context())?.with_accept(Accept::any()),
        RetrievalContext::Default => broker.with_accept(Accept::any()),
    };
    let retrieved = reader.with_responses().registrations().retrieve(id).send().await;
    verify_that!(&retrieved, answered(StatusCode::OK))?;
    let body = json_body(&retrieved?)?;
    let term = |name: &str| -> Result<String> {
        match context {
            RetrievalContext::Link => Ok(name.to_owned()),
            RetrievalContext::Default => Ok(namespace.expanded_term(name)?),
        }
    };
    let parking = term("OffStreetParking")?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "ContextSourceRegistration",
            "endpoint": endpoint.as_str(),
            "information": [
                {
                    "entities": [{"id": namespace.urn_text("Vehicle", "A456"), "type": term("Vehicle")?}],
                    "propertyNames": [term("brandName")?, term("speed")?],
                    "relationshipNames": [term("isParked")?]
                },
                {
                    "entities": [
                        {"idPattern": offstreet_pattern(namespace, ".*downtown$")?.as_ref(), "type": parking},
                        {"idPattern": offstreet_pattern(namespace, ".*47$")?.as_ref(), "type": parking}
                    ],
                    "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                    "relationshipNames": ["isNextToBuilding"]
                }
            ],
        }))
        .hiding_everywhere("@context")
        .hiding_everywhere("mode")
        .hiding_everywhere("operations")
    );
    delete_registrations(&broker, namespace, &[1]).await
}

/// The JSON-LD context a Retrieve Context Source Registration case reads the registration in.
#[derive(Debug, Clone, Copy)]
pub(crate) enum RetrievalContext {
    /// The vocabulary, named in a `Link` header: the vocabulary's terms come back compacted.
    Link,
    /// No `Link` header: the core context, under which the vocabulary's terms come back expanded.
    Default,
}

/// `Delete Created Context Source Registrations`: deletes `urn:ngsi-ld:ContextSourceRegistration:<ns>-<n>` for each
/// `n` of `suffixes` with `client`, requiring 204 for each.
pub(crate) async fn delete_registrations(client: &BrokerClient, namespace: &Namespace, suffixes: &[u32]) -> Result<()> {
    for suffix in suffixes {
        let id: RegistrationId = namespace.urn_text("ContextSourceRegistration", suffix).parse()?;
        let deleted = client.with_responses().registrations().delete(&id).send().await;
        verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;
    }
    Ok(())
}

/// The [`building_registration`] `suffix` at `endpoint` as a query answers it in the vocabulary's context, without
/// `@context`.
pub(crate) fn building_registration_json(namespace: &Namespace, suffix: u32, endpoint: &url::Url) -> Value {
    json!({
        "id": namespace.urn_text("ContextSourceRegistration", suffix),
        "type": "ContextSourceRegistration",
        "information": [{
            "entities": [{"type": "Building"}],
            "propertyNames": ["name", "subCategory"],
            "relationshipNames": ["locatedAt"]
        }],
        "endpoint": endpoint.as_str(),
    })
}

/// `Create Entity` with neither a `Link` nor an `@context`: posts `entity` to `/entities/` as `application/json`,
/// requires 201, and answers the response.
pub(crate) async fn create_entity_plain(broker: &BrokerClient, entity: &Entity<Input>) -> Result<Response<Bytes>> {
    let answer = send_to(broker, &CreateEntity::builder().entity(entity).build(), "entities/").await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    Ok(answer?)
}

/// `Batch Entity Create` of `entities` with `client`, requiring 201: answers the ids the broker reports as created.
pub(crate) async fn batch_create(client: &BrokerClient, entities: &[Entity<Input>]) -> Result<Vec<EntityId>> {
    let created = match client.entities().batch_create(entities).send().await? {
        BatchCreateOutcome::Created(ids) => Some(ids),
        BatchCreateOutcome::Partial(_) => None,
    };
    created.or_fail()
}

/// `Batch Entity Create` of `entities` with `client`, requiring 207: answers the partial result the broker reports.
pub(crate) async fn batch_create_partial(client: &BrokerClient, entities: &[Entity<Input>]) -> Result<BatchOperationResult> {
    let partial = match client.entities().batch_create(entities).send().await? {
        BatchCreateOutcome::Partial(result) => Some(result),
        BatchCreateOutcome::Created(_) => None,
    };
    partial.or_fail()
}

/// Sends `request` with `client`, requires 207, and answers the partial result the broker reports.
pub(crate) async fn partial_batch_answer(client: &BrokerClient, request: UncheckedRequest) -> Result<BatchOperationResult> {
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::MULTI_STATUS))?;
    Ok(serde_json::from_slice(answer?.body())?)
}

/// Requires the partial result of a batch in which exactly the entities `succeeded` were treated and `failed` is the
/// one entity in error, with a non-empty problem.
pub(crate) fn expect_partial_batch(result: &BatchOperationResult, succeeded: &[&EntityId], failed: &EntityId) -> Result<()> {
    expect_that!(result.success.len(), eq(succeeded.len()));
    for id in succeeded {
        expect_that!(&result.success, contains(eq(*id)));
    }
    expect_that!(result.errors.len(), eq(1));
    let error = result.errors.first().or_fail()?;
    expect_that!(&error.entity_id, eq(failed));
    expect_that!(&error.error, not(eq(&ProblemDetails::default())));
    Ok(())
}

/// Requires the answer of a typed operation that succeeds without content (a batch delete, merge, update or upsert
/// that reports every entity done, or a subscription delete) to be the 204 with an empty body.
pub(crate) fn expect_batch_done<O: Debug>(answer: Result<Received<O>, ClientError<ReqwestError>>) -> Result<()> {
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))?;
    expect_that!(answer?.body.len(), eq(0));
    Ok(())
}

/// Requires the answer of a batch upsert to be the 201 that lists exactly the entities `created` as created, and no
/// other entity.
pub(crate) fn expect_upsert_created(answer: Result<Received<UpsertOutcome>, ClientError<ReqwestError>>, created: &[&EntityId]) -> Result<()> {
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    let ids = match answer?.outcome {
        UpsertOutcome::Created(ids) => Some(ids),
        UpsertOutcome::Updated | UpsertOutcome::Partial(_) => None,
    };
    let ids = ids.or_fail()?;
    expect_that!(ids.len(), eq(created.len()));
    for id in created {
        expect_that!(&ids, contains(eq(*id)));
    }
    Ok(())
}

/// Requires the answer of a typed batch operation (delete, merge or update) to be the 207 that reports entities in
/// error, and answers the partial result it carries.
pub(crate) fn partial_batch_outcome(answer: Result<Received<BatchOutcome>, ClientError<ReqwestError>>) -> Result<BatchOperationResult> {
    verify_that!(&answer, answered(StatusCode::MULTI_STATUS))?;
    let partial = match answer?.outcome {
        BatchOutcome::Partial(result) => Some(result),
        BatchOutcome::Done => None,
    };
    partial.or_fail()
}

/// Requires the first error of a partial batch result to be `BadRequestData` with a non-empty title: the refusal of an
/// entity whose body is not a valid entity.
pub(crate) fn expect_bad_request_entity(result: &BatchOperationResult) -> Result<()> {
    let error = result.errors.first().or_fail()?;
    expect_that!(&error.error.error_type, eq(&Some(ErrorType::BadRequestData)));
    expect_that!(error.error.title.as_deref(), some(not(eq(""))));
    Ok(())
}

/// `Query Entities` for the entities `ids` of type `entity_type`, sent by `client` with the `@context` and `Accept` it
/// carries, answered as the JSON body of the response.
pub(crate) async fn query_entities_json(client: &BrokerClient, ids: Vec<EntityId>, entity_type: &str) -> Result<Value> {
    let received = client.with_responses().entities().query().ids(ids).entity_type(entity_type).send().await?;
    Ok(json_body(&received)?)
}

/// The answer of a create of `id` that the Hurl files check for the empty body and the `Location` naming the created
/// resource.
pub(crate) fn expect_created(answer: &Response<Bytes>, id: &impl Display) {
    expect_that!(answer.body().len(), eq(0));
    expect_that!(header_value(answer.headers(), &LOCATION), some(contains_substring(id.to_string())));
}

/// The members of a retrieved Context Source Registration Subscription that the ETSI cases leave uncompared: the
/// notification state the broker reports and the `jsonldContext` it records (clause 5.11.2.4).
pub(crate) const REGISTRATION_SUBSCRIPTION_STATE: [&str; 6] = ["status", "lastFailure", "lastNotification", "timesFailed", "timesSent", "jsonldContext"];

/// `body` without the members `names` at its root: the members a case leaves uncompared, removed so the root's other
/// members are counted exactly.
pub(crate) fn without_members(mut body: Value, names: &[&str]) -> Value {
    if let Some(members) = body.as_object_mut() {
        for name in names {
            members.remove(*name);
        }
    }
    body
}

/// `Retrieve Context Source Registration Subscription` of `id` with the `@context` and `Accept` that `client` carries,
/// answered as the JSON body of the response.
pub(crate) async fn registration_subscription_json(client: &BrokerClient, id: &SubscriptionId) -> Result<Value> {
    let received = client.with_responses().registration_subscriptions().retrieve(id).send().await?;
    Ok(json_body(&received)?)
}

/// The subscription `retrieve` reads, once its `notification` member records a delivery (`lastNotification` present),
/// or as read after the [`delivery_deadline`] when it records none, for the caller's assertions to report.
///
/// Clause 5.8.6 updates the delivery members once the notification exchange completes: the endpoint has the request
/// before the broker has the endpoint's answer, so a retrieval right after the endpoint received the notification can
/// precede the update.
pub(crate) async fn once_delivery_recorded<F, Fut>(retrieve: F) -> Result<Value>
where
    F: Fn() -> Fut + Sync,
    Fut: Future<Output = Result<Value>> + Send,
{
    let recorded = eventually(delivery_deadline(), || async {
        match retrieve().await {
            Ok(body) if body.pointer("/notification/lastNotification").is_some() => Poll::Ready(body),
            Ok(body) => Poll::Pending(body.to_string()),
            Err(error) => Poll::Pending(error.to_string()),
        }
    })
    .await;
    match recorded {
        Ok(body) => Ok(body),
        Err(_) => retrieve().await,
    }
}

/// Requires member `name` of `body` to be absent, null, an empty string, the string `None`, or a date: the check the
/// ETSI cases make of a notification instant a broker may not have set yet.
pub(crate) fn expect_absent_or_date(body: &Value, name: &str) {
    match body.get(name) {
        None | Some(Value::Null) => {}
        Some(Value::String(text)) if text.is_empty() || text == "None" => {}
        Some(other) => expect_that!(other.as_str().map(is_ngsild_date), some(eq(true)), "{name} is a date"),
    }
}

/// Requires member `name` of `body` to be absent, null, an empty string, the string `None`, or a number that is not a
/// float: the check the ETSI cases make of a notification count a broker may not have set yet.
pub(crate) fn expect_absent_or_integer(body: &Value, name: &str) {
    match body.get(name) {
        None | Some(Value::Null) => {}
        Some(Value::String(text)) if text.is_empty() || text == "None" => {}
        Some(other) => expect_that!(other.is_i64() || other.is_u64(), eq(true), "{name} is an integer"),
    }
}

/// The `Content-Type` of the error response `answer` failed with, when exactly one field line carries it.
pub(crate) fn problem_content_type<O>(answer: &Result<O, ClientError<ReqwestError>>) -> Option<&str> {
    match answer {
        Err(ClientError::Response(response)) => header_value(response.head().headers(), &CONTENT_TYPE),
        Ok(_) | Err(_) => None,
    }
}

/// `Create Temporal Entity`: posts `evolution` to `/temporal/entities` with the vocabulary as a one-element `@context`
/// list in the body, and requires the creation, not an update of an existing evolution.
pub(crate) async fn create_temporal_entity(broker: &BrokerClient, vocabulary: &Vocabulary, evolution: &TemporalEntity<Input>) -> Result<()> {
    let client = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    match client.temporal().create_or_update(evolution).send().await? {
        TemporalUpsertOutcome::Created { .. } => Ok(()),
        TemporalUpsertOutcome::Updated => fail!("the broker updated an existing temporal evolution instead of creating it"),
    }
}

/// The suite setup of a temporal case: registers `terms` in the namespace, creates `evolution` in their context, and
/// answers the vocabulary.
pub(crate) async fn create_temporal_evolution(
    broker: &BrokerClient,
    namespace: &Namespace,
    terms: &[&str],
    evolution: &TemporalEntity<Input>,
) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, terms).await?;
    create_temporal_entity(broker, &vocabulary, evolution).await?;
    Ok(vocabulary)
}

/// The suite setup of the temporal cases that load the Vehicle of [`vehicle_speed_fuel_level`]: registers the terms
/// `Vehicle`, `speed` and `fuelLevel`, creates that Vehicle in their context, and answers the vocabulary.
pub(crate) async fn create_vehicle_speed_fuel_level(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    create_temporal_evolution(broker, namespace, &VEHICLE_SPEED_FUEL_TERMS, &vehicle_speed_fuel_level(namespace)?).await
}

/// The suite setup of the temporal cases that load the Vehicle of [`vehicle_speed_fuel_level_with_dataset`]: registers
/// the terms `Vehicle`, `speed` and `fuelLevel`, creates that Vehicle in their context, and answers the vocabulary.
pub(crate) async fn create_vehicle_speed_fuel_level_with_dataset(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    create_temporal_evolution(broker, namespace, &VEHICLE_SPEED_FUEL_TERMS, &vehicle_speed_fuel_level_with_dataset(namespace)?).await
}

/// The suite setup of the replace cases: registers `terms` (the case's context, at least the terms of
/// `VEHICLE_PARKED_SPEED_SOURCE_TERMS`), and creates the Vehicle of [`vehicle_parked_speed_sources`] in their context
/// by a create that requires 201, answering the vocabulary.
pub(crate) async fn create_vehicle_parked_speed_sources(broker: &BrokerClient, namespace: &Namespace, terms: &[&str]) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, terms).await?;
    create_entity(broker, &vocabulary, &vehicle_parked_speed_sources(namespace)?).await?;
    Ok(vocabulary)
}

/// The suite setup of the delete cases that load the Vehicle of [`vehicle_brand_parked_speed_gps`]: registers `terms`
/// (the case's context, at least the terms of `VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS`), creates that Vehicle in
/// their context by a create that requires 201, and answers the vocabulary.
pub(crate) async fn create_vehicle_brand_parked_speed_gps(broker: &BrokerClient, namespace: &Namespace, terms: &[&str]) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, terms).await?;
    create_entity(broker, &vocabulary, &vehicle_brand_parked_speed_gps(namespace)?).await?;
    Ok(vocabulary)
}

/// The suite setup of the append and partial-update cases that load the Vehicle of
/// [`vehicle_brand_parked_speed_sources`]: registers `terms`, creates that Vehicle in their context by a create that
/// requires 201, and answers the vocabulary.
pub(crate) async fn create_vehicle_brand_parked_speed_sources(broker: &BrokerClient, namespace: &Namespace, terms: &[&str]) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, terms).await?;
    create_entity(broker, &vocabulary, &vehicle_brand_parked_speed_sources(namespace)?).await?;
    Ok(vocabulary)
}

/// The suite setup of the scope cases: registers the terms `Vehicle` and `fuelLevel`, and creates the Vehicle of
/// [`vehicle_fuel_level`] in their context with `scope` as the list of the two scope names the suite sends. The typed
/// `scope` member writes a Property that the broker keeps only beside an `observedAt`, so the body is the rendered
/// evolution with that list written in its place. Requires 201.
pub(crate) async fn create_vehicle_scoped_fuel(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &["Vehicle", "fuelLevel"]).await?;
    let evolution = vehicle_fuel_level(namespace)?;
    let scope: RawJson = r#"["/Madrid/Gardens/ParqueNorte","/CompanyA/OrganizationB/UnitC"]"#.parse()?;
    let document = UncheckedDocument::try_from(evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let client = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let operation = CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build();
    let request = client.unchecked(&operation)?.document(&document);
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    Ok(vocabulary)
}

/// The suite setup of the temporal cases that load the Vehicle of [`vehicle_parking_name_fuel`]: registers the terms
/// `Vehicle`, `isParkedIn`, `name` and `fuelLevel`, creates that Vehicle in their context, and answers the vocabulary.
pub(crate) async fn create_vehicle_parking_name_fuel(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    create_temporal_evolution(
        broker,
        namespace,
        &["Vehicle", "isParkedIn", "name", "fuelLevel"],
        &vehicle_parking_name_fuel(namespace)?,
    )
    .await
}

/// `DELETE entities/<id>/attrs/<name>?deleteAll=false`, sent as `application/json` in the context `client` carries.
/// The answer is not checked: the cases that delete an attribute only read what the deletion left behind.
pub(crate) async fn delete_attribute(client: &BrokerClient, id: &EntityId, name: &str) -> Result<()> {
    let deletion = request_to(client, Method::DELETE, &format!("entities/{id}/attrs/{name}?deleteAll=false"))?;
    client.send_unchecked(content_type(deletion, "application/json")).await.any_answer();
    Ok(())
}

/// `operation` sent as the ETSI keyword sends it, as `application/json` in the context `client` carries and with
/// `deleteAll=false` after the selection the typed operation writes: the typed operation leaves that default out. The
/// selection must not be `InstanceSelection::All`, which the typed operation already writes as `deleteAll=true`.
pub(crate) fn naming_delete_all_false<O: Operation>(client: &BrokerClient, operation: &O) -> Result<UncheckedRequest> {
    let request = client.unchecked(operation)?;
    let uri = request.as_request().uri();
    let separator = if uri.query().is_some() { '&' } else { '?' };
    let target: Uri = format!("{uri}{separator}deleteAll=false").parse()?;
    Ok(content_type(request.uri(target), "application/json"))
}

/// `GET temporal/entities/<id>?options=temporalValues&timeproperty=deletedAt` in the context `client` carries, without
/// an `Accept` header: requires 200 and answers the body as JSON.
pub(crate) async fn deleted_temporal_values(client: &BrokerClient, id: &EntityId) -> Result<Value> {
    let received = client
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve_values(id)
        .time_property(TemporalProperty::DeletedAt)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// `Merge Entity` of `id` with `fragment`, sent by `client`: requires 204 and an empty body.
pub(crate) async fn merge_entity(client: &BrokerClient, id: &EntityId, fragment: &EntityFragment<Complete>) -> Result<()> {
    let merged = client.with_responses().entities().merge(id, fragment).send().await;
    verify_that!(&merged, answered(StatusCode::NO_CONTENT))?;
    expect_that!(merged?.body.len(), eq(0));
    Ok(())
}

/// `Append Attributes` of `fragment` to `id` with `overwrite`, sent by `client`: requires 204 and an empty body.
pub(crate) async fn append_attributes(client: &BrokerClient, id: &EntityId, fragment: &EntityFragment, overwrite: Overwrite) -> Result<()> {
    let appended = client
        .with_responses()
        .entities()
        .append_attributes(id, fragment)
        .overwrite(overwrite)
        .send()
        .await;
    verify_that!(&appended, answered(StatusCode::NO_CONTENT))?;
    expect_that!(appended?.body.len(), eq(0));
    Ok(())
}

/// `Update Attributes` of `fragment` to `id`, sent by `client`: requires 204 and an empty body.
pub(crate) async fn update_attributes(client: &BrokerClient, id: &EntityId, fragment: &EntityFragment) -> Result<()> {
    let updated = client.with_responses().entities().update_attributes(id, fragment).send().await;
    verify_that!(&updated, answered(StatusCode::NO_CONTENT))?;
    expect_that!(updated?.body.len(), eq(0));
    Ok(())
}

/// `Partial Attribute Update` of `name` on `id` with `attribute`, sent by `client` in `form`: requires 204 and an
/// empty body.
pub(crate) async fn partial_update(
    client: &BrokerClient,
    id: &EntityId,
    name: &AttributeName,
    attribute: Change<Attribute<Patch>, InstanceDeletion>,
    form: Lossless,
) -> Result<()> {
    let updated = client.with_responses().entities().update_attribute(id, name, attribute).form(form).send().await;
    verify_that!(&updated, answered(StatusCode::NO_CONTENT))?;
    expect_that!(updated?.body.len(), eq(0));
    Ok(())
}

/// The `Partial Attribute Update` of `name` on `id` as `client` sends it with `document` as its body, in the context
/// `client` carries. The typed operation supplies the target and the headers. The attribute it names is a placeholder,
/// since `document` replaces the body it would write.
pub(crate) fn partial_update_request(client: &BrokerClient, id: &EntityId, name: &AttributeName, document: &UncheckedDocument) -> Result<UncheckedRequest> {
    let placeholder = Change::Delete(InstanceDeletion::new(AttributeKind::Relationship));
    let operation = PartialAttributeUpdate::builder().id(id).name(name).attribute(&placeholder).build();
    Ok(client.unchecked(&operation)?.document(document))
}

/// `Partial Attribute Update` of `name` on `id` with `document` as the body, sent by `client` in the context it
/// carries: requires 204 and an empty body.
pub(crate) async fn partial_update_document(client: &BrokerClient, id: &EntityId, name: &AttributeName, document: &UncheckedDocument) -> Result<()> {
    let answer = client.send_unchecked(partial_update_request(client, id, name, document)?).await;
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))?;
    expect_that!(answer?.body().len(), eq(0));
    Ok(())
}

/// The suite setup of the attribute deletion cases: registers the terms of the Building cases and creates the Building
/// of [`building_two_instances`] with `second` as the dataset of its second instances, in a `@context` string.
pub(crate) async fn create_building_instances(broker: &BrokerClient, namespace: &Namespace, second: Option<&str>) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    create_entity_with_context_string(broker, &vocabulary, &building_two_instances(namespace, second)?).await?;
    Ok(vocabulary)
}

/// Registers the terms of the Building cases and creates the Building `urn:ngsi-ld:Vehicle:<ns>-1` of
/// [`building_located_street_geo`] in a `@context` string, answering the vocabulary.
pub(crate) async fn create_building_located_street_geo(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let building = building_located_street_geo(namespace, namespace.urn("Vehicle", 1)?)?;
    create_entity_with_context_string(broker, &vocabulary, &building).await?;
    Ok(vocabulary)
}

/// The `Link` header naming `vocabulary`'s `@context`, for a request whose body carries none.
pub(crate) fn context_link(vocabulary: &Vocabulary) -> Result<HeaderValue> {
    Ok(HeaderValue::from_str(&format!(
        "<{}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"",
        vocabulary.url()
    ))?)
}

/// A `GET` of `target`, relative to the API root and written as the ETSI keyword writes it, refused with 400
/// `BadRequestData` and a non-empty title.
pub(crate) async fn refused_get(broker: &BrokerClient, target: &str) -> Result<()> {
    refused(broker, request_to(broker, Method::GET, target)?, ErrorType::BadRequestData).await
}

/// The suite setup of the linked-entity cases: creates `urn:ngsi-ld:Building:<ns>-<suffix>` located at the City Paris,
/// the City Paris located in France and the Country France, in `vocabulary`'s context, and answers their identifiers
/// in that order.
pub(crate) async fn create_paris_in_france(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace, suffix: &str) -> Result<[EntityId; 3]> {
    create_entity(broker, vocabulary, &building_located_in_paris(namespace, suffix)?).await?;
    create_entity(broker, vocabulary, &city_paris_in_france(namespace)?).await?;
    create_entity(broker, vocabulary, &country_france_described(namespace)?).await?;
    Ok([
        namespace.urn("Building", suffix)?,
        namespace.urn("City", "Paris")?,
        namespace.urn("Country", "France")?,
    ])
}

/// The teardown of a case: `Delete Entity` and `Delete Temporal Entity` for each of `ids`, whose answers are ignored.
pub(crate) async fn delete_entities(broker: &BrokerClient, ids: &[&EntityId]) {
    for id in ids {
        broker.entities().delete(id).send().await.any_answer();
        broker.temporal().delete(id).send().await.any_answer();
    }
}

/// The terms of the `@context` of the Building cases that order by a name and its sub-attribute.
pub(crate) const BUILDING_TERMS: [&str; 3] = ["Building", "name", "subProperty"];

/// The Building `urn:ngsi-ld:Building:<ns>-<n>`, named `name` with a `subProperty` of value `sub`.
pub(crate) fn named_building(namespace: &Namespace, n: u32, name: &str, sub: u32) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", n)?)
        .entity_type("Building")
        .property("name", Property::builder(name).property("subProperty", Property::builder(sub)))
        .build()?)
}

/// Creates the three Buildings in the order 2, 1, 3: named B, A and C, with sub-properties 2, 1 and 3.
pub(crate) async fn create_named_buildings(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    create_entity(broker, vocabulary, &named_building(namespace, 2, "B", 2)?).await?;
    create_entity(broker, vocabulary, &named_building(namespace, 1, "A", 1)?).await?;
    create_entity(broker, vocabulary, &named_building(namespace, 3, "C", 3)?).await
}

/// The teardown of the Building cases: deletes the three Buildings of [`create_named_buildings`].
pub(crate) async fn delete_named_buildings(broker: &BrokerClient, namespace: &Namespace) -> Result<()> {
    delete_entities(
        broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Expects the answer to hold exactly the three Buildings numbered `order`, from the first position to the last.
pub(crate) fn expect_building_order(body: &Value, namespace: &Namespace, order: [u32; 3]) {
    expect_that!(body.as_array().map(Vec::len), some(eq(3)));
    for (position, n) in order.into_iter().enumerate() {
        expect_that!(body.pointer(&format!("/{position}/id")), some(eq(&json!(namespace.urn_text("Building", n)))));
    }
}

/// Sends `request`, requires 200, and answers the body as JSON.
pub(crate) async fn json_answer(broker: &BrokerClient, request: UncheckedRequest) -> Result<Value> {
    let answer = broker.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::OK))?;
    Ok(serde_json::from_slice(answer?.body())?)
}

/// The `NGSILD-Results-Count` header of a query answer, when exactly one field line carries it.
pub(crate) fn results_count<V>(received: &Received<V>) -> Option<&str> {
    header_value(received.head.headers(), &HeaderName::from_static("ngsild-results-count"))
}

/// The `GeoJSON` collection of `features`, with the vocabulary `context` on the collection when `top` is set and on
/// each feature when `on_features` is set: the placements a `GeoJSON` answer under a `Link` context may take.
pub(crate) fn feature_collection(features: [Value; 2], context: &str, top: bool, on_features: bool) -> Value {
    let features = if on_features {
        features.map(|feature| with_feature_context(feature, context))
    } else {
        features
    };
    let mut collection = json!({"type": "FeatureCollection", "features": features});
    if top {
        collection["@context"] = json!(context);
    }
    collection
}

/// Adds the vocabulary `context` to a feature.
fn with_feature_context(mut feature: Value, context: &str) -> Value {
    feature["@context"] = json!(context);
    feature
}

/// A `GET` of `path`, relative to the API root and written with its query as the ETSI keyword writes it, sent with
/// `Accept: application/ld+json`: requires 200 and answers the body as JSON.
pub(crate) async fn get_json(broker: &BrokerClient, path: &str) -> Result<Value> {
    let request = request_to(broker, Method::GET, path)?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    json_answer(broker, request).await
}

/// Sends `request`, requires 400 with the problem type `error_type` and a title.
pub(crate) async fn refused(broker: &BrokerClient, request: UncheckedRequest, error_type: ErrorType) -> Result<()> {
    let answer = broker.send_unchecked(request).await;
    verify_that!(&answer, problem(StatusCode::BAD_REQUEST, error_type).with_title())
}

/// Sends `request`, requires 404 `ResourceNotFound` and a title.
pub(crate) async fn not_found(broker: &BrokerClient, request: UncheckedRequest) -> Result<()> {
    let answer = broker.send_unchecked(request).await;
    verify_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title())
}

/// Sends `request` with `client` and requires `status`, whatever the body: the refusals whose ETSI case names the
/// status only.
pub(crate) async fn answered_with(client: &BrokerClient, request: UncheckedRequest, status: StatusCode) -> Result<()> {
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(status))
}

/// A `POST` of `text` to `path` (relative to the API root, written as the ETSI keyword writes it) sent as
/// `media_type`: the body an invalid case sends exactly as written.
pub(crate) fn post_text(broker: &BrokerClient, path: &str, media_type: &'static str, text: &str) -> Result<UncheckedRequest> {
    Ok(content_type(request_to(broker, Method::POST, path)?, media_type).body(Bytes::copy_from_slice(text.as_bytes())))
}

/// A `POST` with no body to `path` sent as `media_type`, with `Content-Length: 0` named as the case names it: the
/// broker answers 411 to an empty request that names no length.
pub(crate) fn post_empty(broker: &BrokerClient, path: &str, media_type: &'static str) -> Result<UncheckedRequest> {
    Ok(post_text(broker, path, media_type, "")?.append_header(CONTENT_LENGTH, HeaderValue::from_static("0")))
}

/// `request` with the first member separator of its body doubled (`"id":"...",,"type"`), the one defect that makes the
/// invalid-JSON cases' bodies not JSON.
pub(crate) fn with_doubled_separator(request: UncheckedRequest) -> UncheckedRequest {
    let text = String::from_utf8_lossy(request.as_request().body()).replacen(',', ",,", 1);
    request.body(Bytes::from(text))
}

/// `Retrieve Entity` of `id` with the `@context` and `Accept` that `client` carries, answered as the JSON body of the
/// response.
pub(crate) async fn retrieve_entity_json(client: &BrokerClient, id: &EntityId) -> Result<Value> {
    let received = client.with_responses().entities().retrieve(id).send().await?;
    Ok(json_body(&received)?)
}

/// `Retrieve Entity` of `id` with `options=sysAttrs` and the `@context` and `Accept` that `client` carries, answered
/// as the JSON body of the response.
pub(crate) async fn retrieve_entity_sys_attrs_json(client: &BrokerClient, id: &EntityId) -> Result<Value> {
    let received = client
        .with_responses()
        .entities()
        .retrieve(id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    Ok(json_body(&received)?)
}

/// `Retrieve Temporal Evolution` of `id` with the `@context` and `Accept` that `client` carries, answered as the JSON
/// body of the response.
pub(crate) async fn retrieve_temporal_json(client: &BrokerClient, id: &EntityId) -> Result<Value> {
    let received = client.with_responses().temporal().retrieve(id).send().await?;
    Ok(json_body(&received)?)
}

/// `Retrieve Temporal Evolution` of `id` with `options=sysAttrs` and the `@context` and `Accept` that `client`
/// carries, answered as the JSON body of the response.
pub(crate) async fn retrieve_temporal_sys_attrs_json(client: &BrokerClient, id: &EntityId) -> Result<Value> {
    let received = client
        .with_responses()
        .temporal()
        .retrieve(id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    Ok(json_body(&received)?)
}

/// The `instanceId` of the first `speed` instance of a temporal body, which the cases that delete an instance name.
pub(crate) fn first_speed_instance_id(body: &Value) -> Result<InstanceId> {
    Ok(body.pointer("/speed/0/instanceId").and_then(Value::as_str).or_fail()?.parse()?)
}

/// The member `name` of `body` when it is an array, object or string that holds at least one item, character or
/// member, as the `length($.<name>)>0` filter of a `JSONPath` reads it; `None` for any other member or a missing one.
pub(crate) fn non_empty_member<'a>(body: &'a Value, name: &str) -> Option<&'a Value> {
    body.get(name).filter(|member| match member {
        Value::Array(items) => !items.is_empty(),
        Value::Object(members) => !members.is_empty(),
        Value::String(text) => !text.is_empty(),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    })
}

/// The suite setup of the linked-entity cases: creates Building `urn:ngsi-ld:Building:<ns>-<pair>-1` located at City
/// `<pair>-1`, that City, then Building `<pair>-2` located at City `<pair>-2`, and that City, in `vocabulary`'s
/// context.
pub(crate) async fn create_linked_pairs(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace, pair: &str) -> Result<()> {
    create_pairs(broker, vocabulary, namespace, pair, building_located_at_city, city).await
}

/// The suite setup of the linked cases with a street and a location: creates the Buildings and Cities of
/// [`create_linked_pairs`] with the payloads of `building_street_located` and `city_in_france_described`.
pub(crate) async fn create_street_linked_pairs(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace, pair: &str) -> Result<()> {
    create_pairs(broker, vocabulary, namespace, pair, building_street_located, city_in_france_described).await
}

/// Creates Building `<pair>-1` and City `<pair>-1`, then Building `<pair>-2` and City `<pair>-2`, with the payloads
/// `building_payload` and `city_payload` build for the suffix.
async fn create_pairs(
    broker: &BrokerClient,
    vocabulary: &Vocabulary,
    namespace: &Namespace,
    pair: &str,
    building_payload: fn(&Namespace, &str) -> Result<Entity<Input>>,
    city_payload: fn(&Namespace, &str) -> Result<Entity<Input>>,
) -> Result<()> {
    let first = format!("{pair}-1");
    let second = format!("{pair}-2");
    create_entity(broker, vocabulary, &building_payload(namespace, &first)?).await?;
    create_entity(broker, vocabulary, &city_payload(namespace, &first)?).await?;
    create_entity(broker, vocabulary, &building_payload(namespace, &second)?).await?;
    create_entity(broker, vocabulary, &city_payload(namespace, &second)?).await
}

/// The teardown of the linked-entity cases: deletes the Buildings and Cities that [`create_linked_pairs`] creates for
/// `pair`.
pub(crate) async fn delete_linked_pairs(broker: &BrokerClient, namespace: &Namespace, pair: &str) -> Result<()> {
    let first = format!("{pair}-1");
    let second = format!("{pair}-2");
    delete_entities(
        broker,
        &[
            &namespace.urn("Building", &first)?,
            &namespace.urn("City", &first)?,
            &namespace.urn("Building", &second)?,
            &namespace.urn("City", &second)?,
        ],
    )
    .await;
    Ok(())
}

/// The suite setup of the Building and Vehicle cases: creates Building `urn:ngsi-ld:Building:<ns>-1` named Eiffel
/// Tower, Vehicle `urn:ngsi-ld:Vehicle:<ns>-2` parked downtown and Building `urn:ngsi-ld:Building:<ns>-3` named Pisa
/// Tower, in `vocabulary`'s context.
pub(crate) async fn create_eiffel_vehicle_pisa(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    create_entity(broker, vocabulary, &building_simple_attributes(namespace, 1)?).await?;
    create_entity(broker, vocabulary, &vehicle_parked(namespace, 2)?).await?;
    create_entity(broker, vocabulary, &pisa_tower_sightseeing(namespace)?).await
}

/// The teardown of the Building and Vehicle cases: deletes the entities [`create_eiffel_vehicle_pisa`] creates.
pub(crate) async fn delete_eiffel_vehicle_pisa(broker: &BrokerClient, namespace: &Namespace) -> Result<()> {
    delete_entities(
        broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Vehicle", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// The suite setup of the Building, City and Country cases: creates two chains, `<pair>-1` and `<pair>-2`, each a
/// Building `urn:ngsi-ld:Building:<ns>-<suffix>` located at City `<suffix>`, that City named Paris and located in
/// Country `<suffix>` named France, in `vocabulary`'s context.
pub(crate) async fn create_building_city_country_pairs(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace, pair: &str) -> Result<()> {
    for suffix in [format!("{pair}-1"), format!("{pair}-2")] {
        create_entity(broker, vocabulary, &building_located_in_city(namespace, &suffix, &suffix)?).await?;
        create_entity(broker, vocabulary, &city_paris_in_country(namespace, &suffix, &suffix)?).await?;
        create_entity(broker, vocabulary, &country_described(namespace, &suffix)?).await?;
    }
    Ok(())
}

/// The teardown of the cases of [`create_building_city_country_pairs`]: deletes the Buildings, Cities and Countries
/// that it creates for `pair`.
pub(crate) async fn delete_building_city_country_pairs(broker: &BrokerClient, namespace: &Namespace, pair: &str) -> Result<()> {
    for suffix in [format!("{pair}-1"), format!("{pair}-2")] {
        delete_entities(
            broker,
            &[
                &namespace.urn("Building", &suffix)?,
                &namespace.urn("City", &suffix)?,
                &namespace.urn("Country", &suffix)?,
            ],
        )
        .await;
    }
    Ok(())
}

/// The suite setup of the cases that load Vehicles `<suffix>-A` observed on 2020-08-01 and `<suffix>-B` observed on
/// 2020-09-01: registers the terms `Vehicle`, `speed` and `fuelLevel`, creates both in their context, and answers the
/// vocabulary.
pub(crate) async fn create_vehicle_pair(broker: &BrokerClient, namespace: &Namespace, suffix: &str) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &VEHICLE_SPEED_FUEL_TERMS).await?;
    create_vehicle_entities(broker, namespace, &vocabulary, suffix).await?;
    Ok(vocabulary)
}

/// The suite setup of the cases that load the Vehicle pair of [`create_vehicle_pair`] and Bus `<bus_suffix>` observed
/// on 2020-08-01: registers the terms `Vehicle`, `speed`, `fuelLevel` and `Bus`, creates the three in their context,
/// and answers the vocabulary.
pub(crate) async fn create_vehicle_pair_and_bus(broker: &BrokerClient, namespace: &Namespace, suffix: &str, bus_suffix: &str) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &["Vehicle", "speed", "fuelLevel", "Bus"]).await?;
    create_vehicle_pair_and_bus_in(broker, &vocabulary, namespace, suffix, bus_suffix).await?;
    Ok(vocabulary)
}

/// The entities of [`create_vehicle_pair_and_bus`], created in the context of `vocabulary` for a case whose vocabulary
/// names further terms.
pub(crate) async fn create_vehicle_pair_and_bus_in(
    broker: &BrokerClient,
    vocabulary: &Vocabulary,
    namespace: &Namespace,
    suffix: &str,
    bus_suffix: &str,
) -> Result<()> {
    create_vehicle_entities(broker, namespace, vocabulary, suffix).await?;
    create_temporal_entity(broker, vocabulary, &bus_speed_fuel_level(namespace, bus_suffix)?).await
}

/// Creates Vehicle `<suffix>-A` observed on 2020-08-01 and Vehicle `<suffix>-B` observed on 2020-09-01 in the context
/// of `vocabulary`.
async fn create_vehicle_entities(broker: &BrokerClient, namespace: &Namespace, vocabulary: &Vocabulary, suffix: &str) -> Result<()> {
    create_temporal_entity(
        broker,
        vocabulary,
        &vehicle_speed_fuel_level_on(namespace, format!("{suffix}-A"), "2020-08-01")?,
    )
    .await?;
    create_temporal_entity(
        broker,
        vocabulary,
        &vehicle_speed_fuel_level_on(namespace, format!("{suffix}-B"), "2020-09-01")?,
    )
    .await
}

/// The teardown of a temporal case: `Delete Temporal Entity` for each of `ids`, whose answers are ignored.
pub(crate) async fn delete_temporal_entities(broker: &BrokerClient, ids: &[&EntityId]) {
    for id in ids {
        broker.temporal().delete(id).send().await.any_answer();
    }
}

/// The teardown of the cases of [`create_vehicle_pair`]: deletes the temporal evolutions of Vehicles `<suffix>-A` and
/// `<suffix>-B`.
pub(crate) async fn delete_vehicle_pair(broker: &BrokerClient, namespace: &Namespace, suffix: &str) -> Result<()> {
    delete_temporal_entities(
        broker,
        &[
            &namespace.urn("Vehicle", format!("{suffix}-A"))?,
            &namespace.urn("Vehicle", format!("{suffix}-B"))?,
        ],
    )
    .await;
    Ok(())
}

/// The teardown of the cases of [`create_vehicle_pair_and_bus`]: deletes the temporal evolutions of Vehicles
/// `<suffix>-A` and `<suffix>-B` and of Bus `<bus_suffix>`.
pub(crate) async fn delete_vehicle_pair_and_bus(broker: &BrokerClient, namespace: &Namespace, suffix: &str, bus_suffix: &str) -> Result<()> {
    delete_vehicle_pair(broker, namespace, suffix).await?;
    delete_temporal_entities(broker, &[&namespace.urn("Bus", bus_suffix)?]).await;
    Ok(())
}

/// The suite setup of the temporal query cases that page through the Vehicle `<ns>-1` of [`vehicle_twenty_instances`]
/// and the Vehicle `<ns>-2` observed on 2020-09-01: registers the terms `Vehicle`, `speed` and `fuelLevel`, creates
/// both Vehicles in their context, and answers the vocabulary.
pub(crate) async fn create_vehicle_twenty_pair(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &VEHICLE_SPEED_FUEL_TERMS).await?;
    create_temporal_entity(broker, &vocabulary, &vehicle_twenty_instances(namespace)?).await?;
    create_temporal_entity(broker, &vocabulary, &vehicle_speed_fuel_level_on(namespace, 2, "2020-09-01")?).await?;
    Ok(vocabulary)
}

/// The teardown of the cases of [`create_vehicle_twenty_pair`]: deletes the temporal evolutions of Vehicles `<ns>-1`
/// and `<ns>-2`.
pub(crate) async fn delete_vehicle_twenty_pair(broker: &BrokerClient, namespace: &Namespace) -> Result<()> {
    delete_temporal_entities(broker, &[&namespace.urn("Vehicle", 1)?, &namespace.urn("Vehicle", 2)?]).await;
    Ok(())
}

/// A temporal `Content-Range` as written: `<unit> <start>-<end>/<size>`, each part as text.
pub(crate) struct ContentRangeText<'a> {
    /// The unit, `DateTime` for a temporal answer.
    pub(crate) unit: &'a str,
    /// The bound the answer names first: the least recent instant of a forwards answer, the most recent of a backwards
    /// one.
    pub(crate) start: &'a str,
    /// The bound the answer names last.
    pub(crate) end: &'a str,
    /// `*` for a forwards answer, or the `lastN` of a backwards one.
    pub(crate) size: &'a str,
}

impl<'a> ContentRangeText<'a> {
    /// Splits `value` at its unit, at the `Z-` between the bounds and at the last `/`; `None` when it has no such shape.
    fn parse(value: &'a str) -> Option<Self> {
        let (unit, rest) = value.split_once(' ')?;
        let (range, size) = rest.rsplit_once('/')?;
        let divider = range.find("Z-")?;
        Some(Self {
            unit,
            start: range.get(..=divider)?,
            end: range.get(divider + 2..)?,
            size,
        })
    }
}

/// The `Content-Range` of a partial answer, when the header is present once and reads as text.
pub(crate) fn content_range(headers: &HeaderMap) -> Option<ContentRangeText<'_>> {
    header_value(headers, &CONTENT_RANGE).and_then(ContentRangeText::parse)
}

/// How an attribute of a retrieved evolution carries its instants.
#[derive(Clone, Copy)]
pub(crate) enum InstantForm {
    /// Each instance is an object with an `observedAt` member.
    ObservedAtMember,
    /// The simplified representation: `values` holds `[value, observedAt]` pairs.
    ValuePair,
}

impl InstantForm {
    /// The `observedAt` text of `instance`, which the form holds as a member or as the second item of a pair.
    fn observed_at(self, instance: &Value) -> Option<&str> {
        match self {
            Self::ObservedAtMember => instance.get("observedAt")?.as_str(),
            Self::ValuePair => instance.get(1)?.as_str(),
        }
    }
}

/// The list that carries the instances of `attribute` in `body`: the attribute itself, or its `values` in the
/// simplified form.
pub(crate) fn instance_list<'a>(body: &'a Value, attribute: &str, form: InstantForm) -> Option<&'a Value> {
    let attribute = body.get(attribute)?;
    match form {
        InstantForm::ObservedAtMember => Some(attribute),
        InstantForm::ValuePair => attribute.get("values"),
    }
}

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` as the broker retrieves it after [`building_simple_attributes`].
pub(crate) fn simple_building_json(namespace: &Namespace, n: u32, context: &str) -> Value {
    json!({
        "id": namespace.urn_text("Building", n),
        "type": "Building",
        "name": {"type": "Property", "value": "Eiffel Tower"},
        "subCategory": {"type": "Property", "value": "tourism"},
        "airQualityLevel": {
            "type": "Property",
            "value": 4,
            "unitCode": "C62",
            "observedAt": "2020-09-09T16:40:00.000Z"
        },
        "almostFull": {"type": "Property", "value": false},
        "@context": context,
    })
}

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` as the broker retrieves it after [`building_with_weather_sensor`].
pub(crate) fn weather_building_json(namespace: &Namespace, n: u32, context: &str) -> Value {
    json!({
        "id": namespace.urn_text("Building", n),
        "type": "Building",
        "name": {"type": "Property", "value": "Eiffel Tower"},
        "subCategory": {"type": "Property", "value": "tourism"},
        "airQualityLevel": {
            "type": "Property",
            "value": 4,
            "unitCode": "C62",
            "observedAt": "2020-09-09T16:40:00.000Z",
            "hasWeatherObserved": {
                "type": "Relationship",
                "object": namespace.urn_text("Sensor", "43a1af27-4aa7-44fe-bee9-fe76d56978a4")
            }
        },
        "almostFull": {"type": "Property", "value": false},
        "@context": context,
    })
}

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` of [`simple_building_json`] with the `locatedAt` Relationship to the
/// City Paris that a batch writes.
pub(crate) fn located_building_json(namespace: &Namespace, n: u32, context: &str) -> Value {
    let mut building = simple_building_json(namespace, n, context);
    building["locatedAt"] = json!({"type": "Relationship", "object": namespace.urn_text("City", "Paris")});
    building
}

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` with only its `id`, `type` and `@context`, as the broker retrieves it
/// after an entity of that shape is created.
pub(crate) fn bare_building_json(namespace: &Namespace, n: u32, context: &str) -> Value {
    json!({
        "id": namespace.urn_text("Building", n),
        "type": "Building",
        "@context": context,
    })
}

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` with only its `id`, `type`, `@context` and the `locatedAt`
/// Relationship to the City Paris, as the broker retrieves it after the Paris Building of the linked cases is written.
pub(crate) fn bare_located_building_json(namespace: &Namespace, n: u32, context: &str) -> Value {
    let mut building = bare_building_json(namespace, n, context);
    building["locatedAt"] = json!({"type": "Relationship", "object": namespace.urn_text("City", "Paris")});
    building
}

/// The entity `urn:ngsi-ld:Building:<ns>-<n>` with only its `id`, `type` (the `types` in the order given) and
/// `@context`.
pub(crate) fn typed_building_json(namespace: &Namespace, n: u32, types: &[&str], context: &str) -> Value {
    json!({
        "id": namespace.urn_text("Building", n),
        "type": types,
        "@context": context,
    })
}

/// The notification of a subscription in the simplified form, delivered to `uri` as `application/json`.
pub(crate) fn simplified_notification(uri: &url::Url) -> Result<NotificationParams<Input>> {
    let endpoint = Endpoint::builder(uri.as_str()).accept(NgsiLdMediaType::Json).build()?;
    Ok(NotificationParams::builder(endpoint).format(Representation::Simplified).build()?)
}

/// Creates the Building subscription `id` with a `timeInterval` of 5 seconds and a keyValues notification to `uri`, in
/// the context `client` carries, requiring 201; answers the cleanup that deletes it.
pub(crate) async fn create_timed_building_subscription(client: &BrokerClient, id: &SubscriptionId, uri: &url::Url) -> Result<Cleanup> {
    let subscription = Subscription::builder()
        .id(id.clone())
        .time_interval(5)
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(simplified_notification(uri)?)
        .build()?;
    create_subscription_document(client, &subscription, &key_values_document(&subscription, uri)?).await
}

/// Creates the Building subscription `id` with `isActive` false, no time interval and a keyValues notification to
/// `uri`, in the context `client` carries, requiring 201; answers the cleanup that deletes it.
pub(crate) async fn create_paused_building_subscription(client: &BrokerClient, id: &SubscriptionId, uri: &url::Url) -> Result<Cleanup> {
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(simplified_notification(uri)?)
        .build()?;
    let is_active: RawJson = "false".parse()?;
    let document = key_values_document(&subscription, uri)?.set_member("isActive", &is_active);
    create_subscription_document(client, &subscription, &document).await
}

/// The suite setup of the query cases: registers a Context Source Registration and deletes it again, registers the
/// terms `Building`, `name` and `subCategory`, and creates the subscriptions `<ns>-1` (timed), `<ns>-2` (watching
/// `name` and `subCategory`) and `<ns>-3` (paused) in the tenant, answering the vocabulary and their identifiers in
/// that order, and the cleanup that deletes the three subscriptions.
pub(crate) async fn create_query_subscriptions(
    tenant: &BrokerClient,
    namespace: &Namespace,
    endpoint_url: &url::Url,
) -> Result<(Vocabulary, [SubscriptionId; 3], Cleanup)> {
    let registration_id: RegistrationId = namespace.urn_text("ContextSourceRegistration", "provision").parse()?;
    let registration = CSourceRegistration::builder(endpoint_url.as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities(["ProvisioningSentinel"]).build()?])
        .build()?;
    let _provisioning = Cleanup::of(tenant).registration(&registration_id);
    let registered = send_to(tenant, &RegisterContextSource { registration: &registration }, "csourceRegistrations").await;
    verify_that!(&registered, answered(StatusCode::CREATED))?;
    let deleted = tenant.with_responses().registrations().delete(&registration_id).send().await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;

    let vocabulary = Vocabulary::register(tenant, namespace, &["Building", "name", "subCategory"]).await?;
    let client = ld_json_body_client(tenant, &vocabulary)?;
    let [first, second, third] = [1, 2, 3].map(|n| namespace.urn_text("Subscription", n).parse::<SubscriptionId>());
    let (first, second, third) = (first?, second?, third?);
    let cleanup = create_timed_building_subscription(&client, &first, endpoint_url).await?;
    let watched = Subscription::builder()
        .id(second.clone())
        .watched_attributes(["name", "subCategory"])
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(simplified_notification(endpoint_url)?)
        .build()?;
    let cleanup = cleanup
        .absorbing(create_subscription_document(&client, &watched, &key_values_document(&watched, endpoint_url)?).await?)
        .absorbing(create_paused_building_subscription(&client, &third, endpoint_url).await?);
    Ok((vocabulary, [first, second, third], cleanup))
}

/// The body of an `Update Subscription` whose only change is a keyValues `notification` to `uri`, written as the ETSI
/// cases write it: the `type` the fragment writes, then the notification.
pub(crate) fn key_values_fragment(uri: &url::Url) -> Result<UncheckedDocument> {
    Ok(UncheckedDocument::try_from(SubscriptionFragment::builder().build()?.render())?.set_member("notification", &key_values_notification(uri)?))
}

/// Requires the answer to an `Update Subscription` sent as `request` to be the 204 with an empty body.
pub(crate) async fn subscription_updated(client: &BrokerClient, request: UncheckedRequest) -> Result<()> {
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))?;
    expect_that!(answer?.body().len(), eq(0));
    Ok(())
}

/// `Update Subscription` of `id` with `document` as its body, sent by `client` in the context it carries. The typed
/// operation supplies the target and the headers; the empty fragment it names is replaced by `document`.
pub(crate) fn update_subscription_request(client: &BrokerClient, id: &SubscriptionId, document: &UncheckedDocument) -> Result<UncheckedRequest> {
    let placeholder = SubscriptionFragment::builder().build()?;
    Ok(client.unchecked(&UpdateSubscription { id, fragment: &placeholder })?.document(document))
}

/// `Update Context Source Registration Subscription` of `id` with `document` as its body, sent by `client` in the
/// context it carries. The typed operation supplies the target and the headers; the empty fragment it names is
/// replaced by `document`.
pub(crate) fn update_registration_subscription_request(client: &BrokerClient, id: &SubscriptionId, document: &UncheckedDocument) -> Result<UncheckedRequest> {
    let placeholder = SubscriptionFragment::registration_builder().build()?;
    Ok(client.unchecked(&UpdateSubscription { id, fragment: &placeholder })?.document(document))
}

/// The body of an `Update Context Source Registration Subscription` whose only member is a keyValues `notification`
/// to `uri`, written as the ETSI cases write it.
pub(crate) fn registration_notification_fragment(uri: &url::Url) -> Result<UncheckedDocument> {
    let fragment = SubscriptionFragment::registration_builder().build()?;
    Ok(UncheckedDocument::try_from(fragment.render())?
        .remove_member("type")
        .set_member("notification", &key_values_notification(uri)?))
}

/// A pagination link of a query answer as the comparison reads it: its target keeps the path and the `limit` and
/// `offset` parameters, and the relation types and parameters of the link stay as they are.
#[derive(Debug, PartialEq)]
pub(crate) struct PageLink {
    /// The target, with the query reduced to its `limit` and `offset` parameters.
    target: IriRefBuf,
    /// The relation types of the `rel` parameter, in order.
    rels: Vec<RelationType>,
    /// The `type` parameter as text, when present.
    media_type: Option<String>,
    /// The `profile` parameter, when present.
    profile: Option<IriBuf>,
}

impl PageLink {
    /// The link `value` reads as, with its target reduced by [`limit_and_offset`].
    fn reduced(value: &LinkValue) -> Result<PageLink> {
        Ok(PageLink {
            target: limit_and_offset(value.target())?,
            // The comparison owns the links it reads, so each parameter is copied out of the header value.
            rels: value.rels().to_vec(),
            media_type: value.media_type().map(|media_type| media_type.as_str().to_owned()),
            profile: value.profile().cloned(),
        })
    }
}

/// The pagination link of `rel` to `target`, which names the `limit` and `offset` of the page it leads to, with the
/// `type` parameter `media_type` the case expects.
pub(crate) fn page_link(target: &str, rel: RelationType, media_type: &str) -> Result<PageLink> {
    Ok(PageLink {
        target: IriRefBuf::new(target.to_owned())?,
        rels: vec![rel],
        media_type: Some(media_type.to_owned()),
        profile: None,
    })
}

/// `target` with its query reduced to the `limit` and `offset` parameters, in the order written.
fn limit_and_offset(target: &IriRefBuf) -> Result<IriRefBuf> {
    let text = target.as_str();
    let reduced = match text.split_once('?') {
        None => text.to_owned(),
        Some((path, query)) => {
            let kept = query
                .split('&')
                .filter(|pair| matches!(pair.split_once('=').map_or(*pair, |(key, _)| key), "limit" | "offset"))
                .collect::<Vec<_>>()
                .join("&");
            if kept.is_empty() { path.to_owned() } else { format!("{path}?{kept}") }
        }
    };
    Ok(IriRefBuf::new(reduced)?)
}

/// Whether a link-value is a frame the pagination comparison leaves out: the first and last pages, and the JSON-LD
/// context.
fn is_frame(value: &LinkValue) -> bool {
    value.is_context_link() || value.has_rel(&RelationType::First) || value.has_rel(&RelationType::Last)
}

/// The pagination links of the `Link` field of `headers` that remain once the frames are left out, each reduced by
/// [`PageLink::reduced`]. A field sent on several lines is refused, since the comparison reads one field value.
fn page_links(headers: &HeaderMap) -> Result<Vec<PageLink>> {
    let fields = header_values(headers, &LINK);
    verify_that!(fields.len(), le(1))?;
    let Some(text) = fields.first() else {
        return Ok(Vec::new());
    };
    let link = text.parse::<Link>()?;
    link.values().iter().filter(|value| !is_frame(value)).map(PageLink::reduced).collect()
}

/// Requires the `Link` field of `headers` to be free of backslashes, of `'''` and not to end with `'`, as the ETSI
/// keyword reads it, and its pagination links to be `expected`, in any order, each compared by its target's `limit`
/// and `offset`.
pub(crate) fn expect_page_links(headers: &HeaderMap, expected: &[PageLink]) -> Result<()> {
    if let Some(text) = header_value(headers, &LINK) {
        expect_that!(text, not(contains_substring("\\")));
        expect_that!(text, not(contains_substring("'''")));
        expect_that!(text, not(ends_with("'")));
    }
    let links = page_links(headers)?;
    expect_that!(links.len(), eq(expected.len()));
    for link in expected {
        expect_that!(&links, contains(eq(link)));
    }
    Ok(())
}

/// The port of the MQTT listener that an endpoint leaves out.
const MQTT_DEFAULT_PORT: u16 = 1883;

/// The Mosquitto configuration that the broker of an MQTT endpoint runs with.
#[derive(Debug, Clone, Copy)]
pub(crate) enum MqttConfiguration {
    /// `mosquitto.conf`: anonymous clients allowed.
    Anonymous,
    /// `mosquitto_with_user.conf`: the suite's password file, anonymous clients refused.
    WithUser,
}

/// The MQTT listener port that an endpoint names.
#[derive(Debug, Clone, Copy)]
pub(crate) enum MqttListener {
    /// 1883, which the endpoint leaves out.
    Default,
    /// The suite's non-default listener port, which the endpoint names.
    NonDefault,
}

/// The protocol version that the `MQTT-Version` setting of an endpoint names.
#[derive(Debug, Clone, Copy)]
pub(crate) enum MqttVersion {
    /// MQTT 3.1.1, written `mqtt3.1.1`.
    V311,
    /// MQTT 5.0, written `mqtt5.0`.
    V50,
}

/// A `notifierInfo` setting of an MQTT endpoint (clause 7.2).
#[derive(Debug, Clone, Copy)]
pub(crate) enum MqttSetting {
    /// `MQTT-QoS`: the quality of service the notification is published with (Table 7.2-1).
    Qos(QoS),
    /// `MQTT-Version`: the protocol version the notification is published with.
    Version(MqttVersion),
}

impl MqttSetting {
    /// The key and the value of the `notifierInfo` pair the setting writes.
    const fn pair(self) -> (&'static str, &'static str) {
        match self {
            Self::Qos(QoS::AtMostOnce) => ("MQTT-QoS", "0"),
            Self::Qos(QoS::AtLeastOnce) => ("MQTT-QoS", "1"),
            Self::Qos(QoS::ExactlyOnce) => ("MQTT-QoS", "2"),
            Self::Version(MqttVersion::V311) => ("MQTT-Version", "mqtt3.1.1"),
            Self::Version(MqttVersion::V50) => ("MQTT-Version", "mqtt5.0"),
        }
    }
}

/// Publishes an attribute update of the Building `urn:ngsi-ld:Building:<ns>-1` through the broker and listener that
/// `configuration` and `listener` name. The subscription's endpoint carries `settings` as `notifierInfo` and
/// `receiver` as `receiverInfo`. Answers the last message the observer received, once one has arrived.
pub(crate) async fn mqtt_notification(
    userinfo: &str,
    configuration: MqttConfiguration,
    listener: MqttListener,
    settings: &[MqttSetting],
    receiver: &[(&str, &str)],
) -> Result<ReceivedMessage> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let mqtt = SuiteConfig::get()?.mqtt()?;
    let host = match configuration {
        MqttConfiguration::Anonymous => mqtt.host(),
        MqttConfiguration::WithUser => mqtt.auth_host(),
    };
    let (authority, listener_port) = match listener {
        MqttListener::Default => (host.to_string(), MQTT_DEFAULT_PORT),
        MqttListener::NonDefault => (format!("{host}:{}", mqtt.non_default_port()), mqtt.non_default_port()),
    };
    let topic = format!("{namespace}/ngsild-test-suite/topic");
    let observer = MqttObserver::subscribe(&host.to_string(), mqtt.observer_port(), listener_port, &topic, namespace.token()).await?;
    let endpoint = receiver
        .iter()
        .fold(
            settings
                .iter()
                .fold(Endpoint::builder(format!("mqtt://{userinfo}{authority}/{topic}")), |builder, setting| {
                    let (key, value) = setting.pair();
                    builder.notifier_info(key, value)
                }),
            |builder, &(key, value)| builder.receiver_info(key, value),
        )
        .build()?;

    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let as_string = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let as_list = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);

    let building = building_simple_attributes(&namespace, 1)?;
    let created = send_to(&as_string, &CreateEntity::builder().entity(&building).build(), "entities/").await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    sleep(Duration::from_secs(1)).await;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    // The selector takes a copy of the building's identifier; the building keeps its own for the update and the teardown.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(NotificationParams::builder(endpoint).build()?)
        .build()?;
    let _subscription = Cleanup::of(&broker).subscription(&id);
    let subscribed = send_to(&as_list, &CreateSubscription { subscription: &subscription }, "subscriptions/").await;
    verify_that!(&subscribed, answered(StatusCode::CREATED))?;
    sleep(Duration::from_secs(1)).await;
    observer.clear().await;

    let fragment = EntityFragment::builder()
        .property(
            "airQualityLevel",
            Property::builder(5).unit_code(UnitCode::C62).observed_at("2020-10-10T16:40:00.000Z".parse()?),
        )
        .build()?;
    let update = UpdateAttributes::builder().id(&building.id).fragment(&fragment).build();
    send_to(&as_list, &update, &format!("entities/{}/attrs/", building.id)).await.any_answer();

    let messages = observer.wait_for(1).await?;
    let message = messages.into_iter().last().or_fail()?;

    broker.subscriptions().delete(&id).send().await.any_answer();
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(message)
}

/// The `observedAt` texts of the instances of `attribute` in `body`, when every instance carries one.
pub(crate) fn instants<'a>(body: &'a Value, attribute: &str, form: InstantForm) -> Option<Vec<&'a str>> {
    instance_list(body, attribute, form)?
        .as_array()?
        .iter()
        .map(|instance| form.observed_at(instance))
        .collect()
}
