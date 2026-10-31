//! The temporal query carries the too wide rule.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, written_temporal_query::get_parameters};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        ErrorType,
        NgsiLdMediaType,
        Property,
        geometry::{Position, Shape},
    },
    query::{geoquery, q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with a time window answers 200 when narrowed by only a type, attrs,
/// q, a geo-query or local=true, and 400 `BadRequestData` (too wide) with only id, `idPattern`, `scopeQ`, id with
/// `idPattern`, or the window alone
/// Clause: 5.7.4.4, 5.5.13
/// Tenant: private (the window-only and `local=true` queries read every temporal entity of the tenant)
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9006_the_temporal_query_carries_the_too_wide_rule() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vehicle_id = namespace.urn("Vehicle", "temporal-narrowed")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property(
            namespace.attribute("speed")?,
            Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let window = || -> Result<_> { Ok(temporal_query::after("2000-01-01T00:00:00Z".parse()?)) };
    json.temporal().query(window()?).entity_type(&vehicle_type).send().await?;
    json.temporal()
        .query(window()?)
        .attrs(vec![format!("speed_{namespace}").parse()?])
        .send()
        .await?;
    json.temporal().query(window()?).q(q::attr(format!("speed_{namespace}")).gt(10)).send().await?;
    json.temporal()
        .query(window()?)
        .geo_q(geoquery::near_max(100, Shape::Point(Position::try_new(1.0, 2.0)?)))
        .send()
        .await?;
    // Nothing but `local=true` narrows this window, so on a populated broker it spans every entity of the tenant, may be
    // cut at 206 (clause 6.3.10) and may hold evolutions the typed reader refuses; only the status is read.
    let local = get_parameters(
        &broker,
        &[("timerel", "after"), ("timeAt", "2000-01-01T00:00:00Z"), ("local", "true")],
        "application/json",
    )
    .await?;
    expect_that!(
        local.map(|response| response.status()).ok(),
        some(any!(eq(StatusCode::OK), eq(StatusCode::PARTIAL_CONTENT)))
    );
    // The temporal query refuses a window narrowed by nothing it counts before sending, so these requests are sent as
    // written.
    let id_text = format!("urn:ngsi-ld:Vehicle:{namespace}-too-wide");
    let pattern_text = format!("urn:ngsi-ld:Vehicle:{namespace}-.*");
    let time = [("timerel", "after"), ("timeAt", "2000-01-01T00:00:00Z")];
    for narrowing in [
        vec![("id", id_text.as_str())],
        vec![("idPattern", pattern_text.as_str())],
        vec![("scopeQ", "/Madrid")],
        vec![("id", id_text.as_str()), ("idPattern", pattern_text.as_str())],
        vec![],
    ] {
        let parameters = [time.as_slice(), narrowing.as_slice()].concat();
        let answer = get_parameters(&broker, &parameters, "application/json").await?;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    }
    Ok(())
}
