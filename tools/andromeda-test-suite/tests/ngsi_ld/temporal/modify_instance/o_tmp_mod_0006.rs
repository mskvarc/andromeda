//! An empty instance id is 405.

use andromeda_test_suite::{answered_status::AnsweredStatus, broker, namespace::Namespace, outcome::Upserted, problem::answered, wire::api_uri};
use googletest::prelude::*;
use http::{Method, StatusCode, header::ALLOW};
use ngsi_ld::{
    http::operation::AddTemporalAttributes,
    model::{
        Attribute,
        Input,
        Property,
        evolution::{TemporalAttributes, TemporalEntity},
    },
};

/// Pins: PATCH on the Attribute path with an empty `instanceId` segment after it is refused with 405 and an Allow
/// header naming DELETE, the one method the Attribute resource serves
/// Clause: 5.6.14.4, 6.22.3.1, 6.21.3, 6.3.2
/// Recorded: clause 5.6.14.4 asks for `BadRequestData` when the target `instanceId` is not present, but the HTTP
/// binding has no Modify Attribute instance resource without an `instanceId`:
/// `/temporal/entities/{entityId}/attrs/{attrId}` (clause 6.21.3) serves only DELETE, and Andromeda answers a
/// trailing slash as that resource and the request as a method it does not serve (IETF RFC 9110, clause 15.5.6)
#[gtest]
#[tokio::test]
async fn o_tmp_mod_0006_an_empty_instance_id_is_405() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;

    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-09-01T13:05:00Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(40)
                .dataset_id(namespace.dataset_id("Dataset", "fuel")?)
                .observed_at("2020-09-01T14:07:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The instance modification names its instance in the path, which an empty segment leaves out, so the request is
    // the body of Add Attributes sent with PATCH to the attribute path as written.
    let mut fragment = TemporalAttributes::<Input>::new();
    fragment.insert(
        "speed".parse()?,
        Attribute::from(Property::builder(129).observed_at("2020-09-01T12:03:00Z".parse()?).build()?),
    )?;
    let request = broker
        .unchecked(&AddTemporalAttributes::builder().id(&vehicle_id).attributes(&fragment).build())?
        .method(Method::PATCH)
        .uri(api_uri(&broker, &format!("temporal/entities/{vehicle_id}/attrs/speed/"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::METHOD_NOT_ALLOWED).with_header_containing(ALLOW, "DELETE"));
    let allow = answer
        .answered_headers()
        .and_then(|headers| headers.get(ALLOW))
        .and_then(|value| value.to_str().ok());
    expect_that!(allow, some(not(contains_substring("PATCH"))));
    Ok(())
}
