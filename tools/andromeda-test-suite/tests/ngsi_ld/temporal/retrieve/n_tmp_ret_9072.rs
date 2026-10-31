//! Retrieve temporal evolution refuses the query only parameters.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    projected_evolution::evolution_names,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Accept, NgsiLdEntityMap},
        operation::{EntityMapRequest, RetrieveTemporalEvolution, TemporalSelection},
    },
    model::{ErrorType, Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::projection::Projection,
};

/// Pins: Retrieve Temporal Evolution of an Entity with type, q, limit, count or csf answers 400 `InvalidRequest`, while
/// lastN with pick answers 200 and entityMap true answers 201 with an NGSILD-EntityMap header
/// Clause: 6.3.20, Table 6.19.3.1-1, 4.12, 5.7.3.3, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9072_retrieve_temporal_evolution_refuses_the_query_only_parameters() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2024-01-01T01:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for query in [
        format!("type={vehicle_type}"),
        "q=speed%3E1".to_owned(),
        "limit=3".to_owned(),
        "count=true".to_owned(),
        "csf=tier%3D%3D1".to_owned(),
    ] {
        // Retrieve defines none of these parameters, so the query string is sent as written.
        let request = request_to(&client, Method::GET, &format!("temporal/entities/{vehicle_id}?{query}"))?
            .append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let answer = client.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    }
    let retrieved = client
        .temporal()
        .retrieve_projected(&vehicle_id)
        .last_n(1)
        .pick("speed".parse::<Projection>()?)
        .send()
        .await?;
    let projected = retrieved.body();
    expect_that!(evolution_names(projected), elements_are![eq("speed")]);
    expect_that!(&projected.entity_type, none());
    let speed = projected.attributes.get(&"speed".parse()?).or_fail()?;
    expect_that!(speed.as_slice(), all!(len(eq(1)), nth(0, any_attribute().value(2))));
    let map_request = EntityMapRequest::returned();
    let operation = RetrieveTemporalEvolution::builder()
        .id(&vehicle_id)
        .selection(TemporalSelection::Unbounded {
            time_property: None,
            last_n: None,
            aggregation_methods: None,
            aggregation_period: None,
        })
        .entity_map(&map_request)
        .format(Lossless::Normalized)
        .build();
    let request = client.unchecked(&operation)?;
    let created = client.send_unchecked(request).await?;
    expect_that!(created.status(), eq(StatusCode::CREATED));
    expect_that!(
        ResponseHead::new(created.headers().clone()).typed::<NgsiLdEntityMap>().into_valid(),
        some(anything())
    );
    Ok(())
}
