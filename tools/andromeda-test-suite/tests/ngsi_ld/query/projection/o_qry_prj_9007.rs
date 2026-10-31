//! Attrs beside pick or omit on a url is bad request data.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    raw_call::target,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, ErrorType, NgsiLdMediaType, Property},
    query::projection::Projection,
};

/// Pins: Retrieve Entity, Query Entities, Retrieve Temporal Evolution of an Entity and Query Temporal Evolution of
/// Entities each answer 400 `BadRequestData` for attrs beside pick or beside omit, and Retrieve Entity with pick,
/// omit or attrs alone answers 200
/// Clause: 4.21, 5.7.1.4, 5.7.2.4, 5.7.3.4, 5.7.4.4
/// Recorded: no clause forbids combining pick or omit with the deprecated attrs, which Tables 6.4.3.2-1, 6.5.3.1-1
/// and 5.2.14.1-1 make a synonym for pick (attributes in a notification); Andromeda refuses attrs beside pick or
/// omit with `BadRequestData`
#[gtest]
#[tokio::test]
async fn o_qry_prj_9007_attrs_beside_pick_or_omit_on_a_url_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "selectors")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // `attrs` beside `pick` or `omit` is an input the client does not write, so each refused read goes out as written.
    let type_text = vehicle_type.to_string();
    let id_path = format!("entities/{vehicle_id}");
    let temporal_id_path = format!("temporal/entities/{vehicle_id}");
    let time = [("timerel", "after"), ("timeAt", "2000-01-01T00:00:00Z")];
    for projection in ["pick", "omit"] {
        let by_id = [(projection, "speed"), ("attrs", "speed")];
        let by_type = [("type", type_text.as_str()), (projection, "speed"), ("attrs", "speed")];
        let temporal_by_id = [time[0], time[1], (projection, "speed"), ("attrs", "speed")];
        let temporal_by_type = [("type", type_text.as_str()), time[0], time[1], (projection, "speed"), ("attrs", "speed")];
        let targets = [
            target(&id_path, &by_id),
            target("entities", &by_type),
            target(&temporal_id_path, &temporal_by_id),
            target("temporal/entities", &temporal_by_type),
        ];
        for read in &targets {
            let answer = broker
                .send_unchecked(exchange(&broker, Method::GET, read, &[("Accept", "application/json")], None)?)
                .await;
            expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
        }
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let picked = json
        .entities()
        .retrieve_projected(&vehicle_id)
        .pick("speed".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(picked.body(), has_attribute("speed", property().value(60)));
    let omitted = json
        .entities()
        .retrieve_projected(&vehicle_id)
        .omit("speed".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(omitted.body(), lacks_attribute("speed"));
    let attrs = json
        .entities()
        .retrieve(&vehicle_id)
        .attrs(vec!["speed".parse::<AttributeName>()?])
        .send()
        .await?;
    expect_that!(attrs.body(), has_attribute("speed", property().value(60)));
    Ok(())
}
