//! A boolean parameter outside lowercase is bad request data on every endpoint.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker::{self, BrokerClient},
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::{UncheckedRequest, headers::Accept, operation::ResultsCount},
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
    query::Locality,
};

/// Sends `request` and expects 400 `BadRequestData`.
async fn expect_refused(broker: &BrokerClient, request: UncheckedRequest) {
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
}

/// `request` asking for `application/json`.
fn accepting_json(request: UncheckedRequest) -> UncheckedRequest {
    request.append_header(ACCEPT, HeaderValue::from_static("application/json"))
}

/// `request` carrying `body` as `application/json`.
fn with_json_body(request: UncheckedRequest, body: &str) -> UncheckedRequest {
    request
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(body.to_owned()))
}

/// Pins: Every Boolean query parameter answers 400 `BadRequestData` when spelt TRUE, True, FALSE or False, on every
/// endpoint that takes one (count, local, `entityMap` and `splitEntities` on Query Entities, local and `entityMap`
/// on Retrieve Entity, count and local on the batch query, local on entity create, replace and delete, `deleteAll`
/// and local on Delete Attribute, details and local on the type and attribute discovery, details and reload on the
/// @context endpoints, count, local and `deleteAll` on the Temporal API), changes nothing, and the lowercase
/// spelling is read
/// Clause: 4.9, 6.3.13, 6.3.18, Table 6.4.3.2-1, Table 6.7.3.2-1, Table 6.25.3.1-1, Table 6.27.3.1-1, Table
/// 6.29.3.2-1, Table 6.30.3.1-1
/// Recorded: the parameter tables type these parameters Boolean and every example writes true or false; clause 4.9
/// binds true and false to the RFC 8259 literals, which are lowercase, and the specification defines no other
/// spelling, so Andromeda reads only true and false and refuses any other letter case as data breaking the
/// parameter's type
#[gtest]
#[tokio::test]
async fn o_cmn_val_9032_a_boolean_parameter_outside_lowercase_is_bad_request_data_on_every_endpoint() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_2_id = namespace.urn("Device", 2)?;
    let device_3_id = namespace.urn("Device", 3)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&device).send().await?.done()?;
    // The client writes only lowercase Boolean parameters, so every refused request is sent as written.
    let entities = format!("entities?type=Device_{namespace}");
    for parameter in ["count=TRUE", "local=True", "entityMap=FALSE", "splitEntities=False"] {
        expect_refused(&broker, accepting_json(request_to(&broker, Method::GET, &format!("{entities}&{parameter}"))?)).await;
    }
    for parameter in ["local=TRUE", "entityMap=TRUE"] {
        expect_refused(
            &broker,
            accepting_json(request_to(&broker, Method::GET, &format!("entities/{device_id}?{parameter}"))?),
        )
        .await;
    }
    let query = format!(r#"{{"type": "Query", "entities": [{{"type": "Device_{namespace}"}}]}}"#);
    for parameter in ["count=TRUE", "local=True"] {
        let request = with_json_body(
            accepting_json(request_to(&broker, Method::POST, &format!("entityOperations/query?{parameter}"))?),
            &query,
        );
        expect_refused(&broker, request).await;
    }
    let created = format!(r#"{{"id": "{device_2_id}", "type": "Device_{namespace}"}}"#);
    expect_refused(&broker, with_json_body(request_to(&broker, Method::POST, "entities?local=TRUE")?, &created)).await;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_2_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let replacement = format!(r#"{{"type": "Device_{namespace}", "speed": {{"type": "Property", "value": 99}}}}"#);
    expect_refused(
        &broker,
        with_json_body(request_to(&broker, Method::PUT, &format!("entities/{device_id}?local=TRUE"))?, &replacement),
    )
    .await;
    expect_refused(
        &broker,
        request_to(&broker, Method::DELETE, &format!("entities/{device_id}/attrs/speed?deleteAll=TRUE"))?,
    )
    .await;
    expect_refused(
        &broker,
        request_to(&broker, Method::DELETE, &format!("entities/{device_id}/attrs/speed?local=True"))?,
    )
    .await;
    expect_refused(&broker, request_to(&broker, Method::DELETE, &format!("entities/{device_id}?local=TRUE"))?).await;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("speed", any_attribute().value(10)));
    for path in [
        "types?details=TRUE".to_owned(),
        "types?local=TRUE".to_owned(),
        format!("types/Device_{namespace}?local=TRUE"),
        "attributes?details=True".to_owned(),
        "attributes?local=TRUE".to_owned(),
        "attributes/speed?local=TRUE".to_owned(),
        "jsonldContexts?details=TRUE".to_owned(),
        format!("jsonldContexts/urn:ngsi-ld:context:{namespace}?details=TRUE"),
    ] {
        expect_refused(&broker, accepting_json(request_to(&broker, Method::GET, &path)?)).await;
    }
    expect_refused(
        &broker,
        request_to(&broker, Method::DELETE, &format!("jsonldContexts/urn:ngsi-ld:context:{namespace}?reload=TRUE"))?,
    )
    .await;
    let temporal = format!("temporal/entities?type=Device_{namespace}&timerel=after&timeAt=2020-01-01T00:00:00Z");
    for parameter in ["count=TRUE", "local=True"] {
        expect_refused(&broker, accepting_json(request_to(&broker, Method::GET, &format!("{temporal}&{parameter}"))?)).await;
    }
    expect_refused(
        &broker,
        accepting_json(request_to(&broker, Method::GET, &format!("temporal/entities/{device_id}?local=TRUE"))?),
    )
    .await;
    let temporal_query = format!(
        r#"{{"type": "Query", "entities": [{{"type": "Device_{namespace}"}}], "temporalQ": {{"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"}}}}"#
    );
    let request = with_json_body(
        accepting_json(request_to(&broker, Method::POST, "temporal/entityOperations/query?local=TRUE")?),
        &temporal_query,
    );
    expect_refused(&broker, request).await;
    let temporal_created = format!(
        r#"{{"id": "{device_3_id}", "type": "Device_{namespace}", "speed": [{{"type": "Property", "value": 30, "observedAt": "2020-08-01T12:00:00Z"}}]}}"#
    );
    expect_refused(
        &broker,
        with_json_body(request_to(&broker, Method::POST, "temporal/entities?local=TRUE")?, &temporal_created),
    )
    .await;
    expect_refused(
        &broker,
        request_to(&broker, Method::DELETE, &format!("temporal/entities/{device_id}/attrs/speed?deleteAll=TRUE"))?,
    )
    .await;
    expect_refused(
        &broker,
        request_to(&broker, Method::DELETE, &format!("temporal/entities/{device_id}?local=TRUE"))?,
    )
    .await;
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&device_3_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&device_type)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(1)));
    expect_that!(page.items(), len(eq(1)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&device_id)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(retrieved_2.body(), has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(10)))));
    broker.entities().delete(&device_id).locality(Locality::Local).send().await?.done()?;
    Ok(())
}
