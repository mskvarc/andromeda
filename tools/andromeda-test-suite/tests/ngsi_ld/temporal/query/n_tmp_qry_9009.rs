//! A temporal ordering by a member other than id is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, written_temporal_query::get};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{Entity, ErrorType};

/// Pins: Query Temporal Evolution of Entities with `orderBy=name` answers 400 `BadRequestData`, with and without
/// local=true
/// Clause: 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9009_a_temporal_ordering_by_a_member_other_than_id_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let order_scope_type = namespace.entity_type("OrderScope")?;
    for (suffix, name) in [("order-1", "A"), ("order-2", "B"), ("order-3", "C")] {
        let building = Entity::builder()
            .id(namespace.urn("Building", suffix)?)
            .entity_type(order_scope_type.clone())
            .property("name", name)
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
    }
    // The temporal query refuses an ordering by a member other than `id` before sending, so the reads are sent as written.
    let answer = get(
        &broker,
        &format!("type=OrderScope_{namespace}&timerel=after&timeAt=1970-01-01T00%3A00%3A00Z&timeproperty=createdAt&orderBy=name"),
        "application/json",
    )
    .await?;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = get(
        &broker,
        &format!("type=OrderScope_{namespace}&timerel=after&timeAt=1970-01-01T00%3A00%3A00Z&timeproperty=createdAt&local=true&orderBy=name"),
        "application/json",
    )
    .await?;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
