//! A posted temporal wildcard selector beside another refuses local false.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
    unchecked_call::exchange,
    unchecked_outcome::read_as,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        UncheckedRequest,
        headers::Accept,
        operation::{QueryMethod, QueryTemporalEvolution},
    },
    model::{ErrorType, Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{builders::SelectorTypeBuilder, temporal_query::TemporalQuery},
};

/// Pins: Query Temporal Evolution of Entities by POST with a type * `EntitySelector` beside a named one answers 400
/// `InvalidRequest` with local false, and 200 with the entities of both selectors alone or with local true
/// Clause: 5.2.33, 6.3.18, 6.3.20, 6.24.3.1
/// Invalid type: * (the every-type selector under test, held to one entity by its ns-scoped id)
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9378_post_a_wildcard_selector_beside_another_refuses_local_false() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "wild-1")?;
    let building_type = namespace.entity_type("Building")?;
    let vehicle_id = namespace.urn("Vehicle", "wild-2")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let building = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("height", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&building).send().await?.created()?;
    let vehicle = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(2).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle).send().await?.created()?;
    // The temporal query names one selector and refuses `*` with local false, so each call goes out as written; the
    // answers are read through a query of the same kind.
    let selector = SelectorTypeBuilder::from(&building_type).build()?;
    let before = TemporalQuery::before("2099-01-01T00:00:00Z".parse()?);
    let query = QueryTemporalEvolution::builder()
        .entity_type(&selector)
        .temporal_q(&before)
        .method(QueryMethod::Post)
        .format(Lossless::Normalized)
        .build();
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let body = format!(
        r#"{{"type": "Query", "entities": [{{"id": "urn:ngsi-ld:Building:{namespace}-wild-1", "type": "Building_{namespace}"}}, {{"id": "urn:ngsi-ld:Vehicle:{namespace}-wild-2", "type": "*"}}], "temporalQ": {{"timerel": "before", "timeAt": "2099-01-01T00:00:00Z"}}}}"#
    );
    let refused = json
        .send_unchecked(posted(&broker, "temporal/entityOperations/query?local=false", &body)?)
        .await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    for target in ["temporal/entityOperations/query", "temporal/entityOperations/query?local=true"] {
        let request = posted(&broker, target, &body)?;
        let sent = request.as_request().clone();
        let response = json.send_unchecked(request).await?;
        expect_that!(response.status(), eq(StatusCode::OK));
        let page = read_as(&query, &sent, response)?;
        expect_that!(page.items(), len(eq(2)));
        expect_that!(page.items(), contains(has_id(&building_id)));
        expect_that!(page.items(), contains(has_id(&vehicle_id)));
    }
    Ok(())
}

/// A POST of `body` to `target` as `application/json`, asking for `application/json`.
fn posted(broker: &BrokerClient, target: &str, body: &str) -> Result<UncheckedRequest> {
    Ok(exchange(
        broker,
        Method::POST,
        target,
        &[("Content-Type", "application/json"), ("Accept", "application/json")],
        Some(body),
    )?)
}
