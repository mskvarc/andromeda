//! A query matching nothing creates an empty map.

use andromeda_test_suite::{broker, namespace::Namespace, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Create `EntityMap` for Query Entities over a type no Entity has answers 201 with an empty `entityMap`
/// Clause: 5.14.4.4, 5.2.39
#[gtest]
#[tokio::test]
async fn n_map_qen_0009_a_query_matching_nothing_creates_an_empty_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&namespace.entity_type("Nothing")?)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(0)));
    Ok(())
}
