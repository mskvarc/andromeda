//! A temporal q on a sub attribute of a respond async create is answered at once.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::answered,
    respond_async::{async_applied, respond_async},
    temporal_page_body::entities,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{BatchCreate, NormalizedTemporal, QueryTemporalEvolution},
    },
    model::{Entity, NgsiLdMediaType, Number, Property, Value},
    query::{builders::SelectorTypeBuilder, q, temporal_query},
};

/// Pins: Query Temporal Evolution with q=speed.accuracy>0.5 sent straight after a Batch Entity Creation answered
/// 201 with Preference-Applied: respond-async wrote speed with accuracy 0.9, with no retry, answers the Entity, and
/// with q=speed.accuracy>0.95 answers nothing
/// Clause: 5.6.7, 5.7.4.4
/// Recorded: Andromeda extension, not v1.9.1: Prefer: respond-async on batch operations (IETF RFC 7240 section
/// 4.1), whose history is queued with the write and answered by every temporal read before the queue is applied;
/// clause 5.7.4.4 step S2 checks the query against the Attribute instances in the window, and the instance a write
/// recorded is one of them from the moment the write is acknowledged
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9074_a_temporal_q_on_a_sub_attribute_of_a_respond_async_create_is_answered_at_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_id = namespace.urn("Car", 1)?;
    let car_type = namespace.entity_type("Car")?;
    let entities_to_create = [Entity::builder()
        .id(car_id.clone())
        .entity_type(car_type.clone())
        .property(
            "speed",
            Property::builder(42)
                .observed_at("2026-03-01T10:00:00Z".parse()?)
                .property("accuracy", Property::builder(Value::Number("0.9".parse::<Number>()?))),
        )
        .build()?];
    let request = respond_async(broker.unchecked(&BatchCreate::builder().entities(&entities_to_create).build())?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, async_applied(StatusCode::CREATED));
    // Asked at once, with no retry.
    let temporal_q = temporal_query::after("2026-01-01T00:00:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&car_type).build()?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for (bound, hits) in [("0.5", 1), ("0.95", 0)] {
        let accurate = q::attr("speed").sub_attr("accuracy").gt(bound.parse::<Number>()?).build()?;
        let query = QueryTemporalEvolution::builder()
            .temporal_q(&temporal_q)
            .entity_type(&selector)
            .q(&accurate)
            .format(NormalizedTemporal)
            .build();
        let page = client.send_unchecked(client.unchecked(&query)?).await;
        expect_that!(&page, answered(StatusCode::OK));
        let selected = entities(&page?)?;
        expect_that!(selected, len(eq(hits)));
        if hits == 1 {
            expect_that!(selected, nth(0, has_id(&car_id)));
        }
    }
    Ok(())
}
