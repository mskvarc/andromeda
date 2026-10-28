//! The registrations, stub rules and payloads the entity-attribute cases share: the Vehicle
//! `urn:ngsi-ld:Vehicle:<ns>-1` registered by id for the redirection operations, the rules of the attribute requests
//! a source receives for it, and the attributes the cases write.

use crate::distributed_operations::{
    fork_rule::ForkRule,
    forwarding_source::{registration_id, source_path},
    redirected_request::{forwarded_body, forwarded_options},
};
use andromeda_test_suite::{
    broker::BrokerClient,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    wire::{content_type, request_to},
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, operation::Overwrite},
    model::{Attribute, AttributeName, DatasetId, Entity, EntityFragment, EntityId, EntityTypeName, Input, Lossless, Property, Relationship},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};
use serde_json::{Value, json};
use url::Url;
use wiremock::Request;

/// `Wait For Request`: waits until the source holds a request beyond the `taken` it held before the case's action,
/// and answers the last request it holds.
pub(crate) async fn wait_for_request(source: &ContextSource, taken: usize) -> Result<Request> {
    source.wait_for(taken + 1).await?.into_iter().last().or_fail()
}

/// Which instances of an attribute a deletion names, as its `deleteAll` parameter writes it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DeletedInstances {
    /// `deleteAll=false`: the default instance alone.
    Default,
    /// `deleteAll=true`: every instance.
    All,
}

impl DeletedInstances {
    /// The `deleteAll` value.
    const fn parameter(self) -> &'static str {
        match self {
            DeletedInstances::Default => "false",
            DeletedInstances::All => "true",
        }
    }
}

/// `Delete Entity Attributes` of the `instances` of `name` on `id`, as `application/json` in the context `client`
/// carries in its `Link` header: requires 204.
pub(crate) async fn delete_attribute_instance(client: &BrokerClient, id: &EntityId, name: &str, instances: DeletedInstances) -> Result<()> {
    let target = format!("entities/{id}/attrs/{name}?deleteAll={}", instances.parameter());
    let deletion = request_to(client, Method::DELETE, &target)?;
    let answer = client.send_unchecked(content_type(deletion, "application/json")).await;
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))
}

/// The registration `urn:ngsi-ld:ContextSourceRegistration:<ns>-<suffix>` of the Vehicle `urn:ngsi-ld:Vehicle:<ns>-1`
/// at `endpoint` for the redirection operations, with the attributes `properties` when it names any, in `mode` when
/// it names one.
pub(crate) fn vehicle_attribute_registration(
    namespace: &Namespace,
    suffix: u32,
    endpoint: &Url,
    mode: Option<RegistrationMode>,
    properties: &[&str],
) -> Result<CSourceRegistration<Input>> {
    let vehicle = [EntityInfo::new("Vehicle".parse::<EntityTypeName>()?).with_id(namespace.urn("Vehicle", 1)?)];
    let information = match properties {
        [] => RegistrationInfo::builder().entities(vehicle).build()?,
        names => RegistrationInfo::builder().entities(vehicle).property_names(names.iter().copied()).build()?,
    };
    Ok(CSourceRegistration::builder(endpoint.as_str())
        .id(registration_id(namespace, suffix)?)
        .information([information])
        .operations([OperationGroup::RedirectionOps])
        .maybe_mode(mode)
        .build()?)
}

/// The stub rule of a `method` request at the source's `<below>/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:<ns>-1<tail>`.
pub(crate) fn vehicle_rule(namespace: &Namespace, method: Method, below: &str, tail: &str) -> ForkRule {
    let path = format!("{below}/ngsi-ld/v1/entities/{}{tail}", namespace.urn_text("Vehicle", 1));
    ForkRule::resource(method, &source_path(namespace, &path))
}

/// `Append Entity Attributes` of `fragment` to `id` with `options=noOverwrite`, sent by `client`: requires 207.
pub(crate) async fn append_without_overwrite(client: &BrokerClient, id: &EntityId, fragment: &EntityFragment) -> Result<()> {
    let appended = client
        .with_responses()
        .entities()
        .append_attributes(id, fragment)
        .overwrite(Overwrite::Keep)
        .send()
        .await;
    verify_that!(&appended, answered(StatusCode::MULTI_STATUS))
}

/// `Get Request Url Params options` of `request` holds `noOverwrite`: one value containing it, or several of which one
/// is it.
pub(crate) fn expect_no_overwrite_forwarded(request: &Request) {
    let options = forwarded_options(request);
    expect_that!(
        options,
        any![
            all![len(eq(1)), contains(contains_substring("noOverwrite"))],
            all![len(gt(1)), contains(eq("noOverwrite"))],
        ]
    );
}

/// The attributes the append cases write: the `isParked` Relationship to the `OffStreetParking` Downtown1, observed at
/// 2017-07-29T12:00:04Z and provided by `JohnDoe`, and a `speed` of 99 from the Speedometer.
pub(crate) fn parked_by_john_doe_and_speed(namespace: &Namespace) -> Result<EntityFragment> {
    let parked = Relationship::builder(namespace.urn("OffStreetParking", "Downtown1")?)
        .observed_at("2017-07-29T12:00:04Z".parse()?)
        .relationship("providedBy", namespace.urn("Person", "JohnDoe")?)
        .build()?;
    Ok(EntityFragment::builder()
        .relationship("isParked", parked)
        .property("speed", Property::builder(99).property("source", "Speedometer"))
        .build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the deletion cases: a Mercedes whose `isParked` Relationship in the dataset
/// `urn:ngsi-ld:Relationship:<ns>-parked12345` is the `OffStreetParking` Downtown1 observed at 2017-07-29T12:00:04Z and
/// provided by Bob, with a default `speed` of 55 and a `speed` of 70 in the dataset
/// `urn:ngsi-ld:Property:<ns>-gpsBxyz123-speed`, both from the Speedometer.
pub(crate) fn vehicle_with_two_speeds(namespace: &Namespace) -> Result<Entity<Input>> {
    let parked = Relationship::builder(namespace.urn("OffStreetParking", "Downtown1")?)
        .observed_at("2017-07-29T12:00:04Z".parse()?)
        .relationship("providedBy", namespace.urn("Person", "Bob")?)
        .dataset_id(namespace.urn_text("Relationship", "parked12345").parse::<DatasetId>()?)
        .build()?;
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("brandName", "Mercedes")
        .relationship("isParked", parked)
        .property("speed", Property::builder(55).property("source", "Speedometer"))
        .property(
            "speed",
            Property::builder(70)
                .property("source", "Speedometer")
                .dataset_id(namespace.urn_text("Property", "gpsBxyz123-speed").parse::<DatasetId>()?),
        )
        .build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the partial update cases: its `isParked2` Relationship is the
/// `OffStreetParking` Downtown2 observed at 2017-07-29T12:00:04Z and provided by Alice, and it holds a `speed` of
/// `speed` from the Speedometer when `speed` names one.
pub(crate) fn vehicle_parked2_by_alice(namespace: &Namespace, speed: Option<u32>) -> Result<Entity<Input>> {
    let parked = Relationship::builder(namespace.urn("OffStreetParking", "Downtown2")?)
        .observed_at("2017-07-29T12:00:04Z".parse()?)
        .relationship("providedBy", namespace.urn("Person", "Alice")?)
        .build()?;
    let vehicle = Entity::builder().id(namespace.urn("Vehicle", 1)?).entity_type("Vehicle");
    let vehicle = match speed {
        Some(speed) => vehicle.property("speed", Property::builder(speed).property("source", "Speedometer")),
        None => vehicle,
    };
    Ok(vehicle.relationship("isParked2", parked).build()?)
}

/// The body the partial update cases send: the `speed` of 99 from the Speedometer as a member named `speed`.
pub(crate) fn speed_patch_document() -> Result<UncheckedDocument> {
    Ok(UncheckedDocument::empty().set_member("speed", &speed_99()?.render(Lossless::Normalized)))
}

/// The `speed` of 99 from the Speedometer the partial update and replace cases write.
pub(crate) fn speed_99() -> Result<Attribute<Input>> {
    Ok(Attribute::from(Property::builder(99).property("source", "Speedometer").build()?))
}

/// `Replace Entity Attribute` of the `speed` of `id` with [`speed_99`], as `application/json` in the context `client`
/// carries in its `Link` header: requires 204.
pub(crate) async fn replace_speed(client: &BrokerClient, id: &EntityId) -> Result<()> {
    let speed: AttributeName = "speed".parse()?;
    let replaced = client.with_responses().entities().replace_attribute(id, &speed, &speed_99()?).send().await;
    verify_that!(&replaced, answered(StatusCode::NO_CONTENT))
}

/// `Get Request Body` of `request` has a member `speed`.
pub(crate) fn expect_speed_forwarded(request: &Request) -> Result<()> {
    expect_that!(forwarded_body(request)?.get("speed"), some(anything()));
    Ok(())
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` a source answers a retrieval with in the replace cases: its `speed` of
/// 99 from the Speedometer.
pub(crate) fn speed_99_reply(namespace: &Namespace) -> Value {
    json!({
        "speed": {"type": "Property", "value": 99, "source": {"type": "Property", "value": "Speedometer"}},
        "id": namespace.urn_text("Vehicle", 1),
        "type": "Vehicle",
    })
}
