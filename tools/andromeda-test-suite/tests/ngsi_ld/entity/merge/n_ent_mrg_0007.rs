//! A patch without an entity id is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::MergeEntity,
    model::{EntityFragment, ErrorType},
};

/// Pins: a PATCH on the entities collection, which names no Entity id, is refused with 400 `BadRequestData`
/// Clause: 5.6.17.4, 6.4.3
#[gtest]
#[tokio::test]
async fn n_ent_mrg_0007_a_patch_without_an_entity_id_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let placeholder_id = namespace.urn("Placeholder", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let fragment = EntityFragment::builder().entity_type(building_type).property("name", "Pisa Tower").build()?;
    // The operation always names an entity in its path, so it is sent to the collection path as written.
    let request = broker
        .unchecked(&MergeEntity::builder().id(&placeholder_id).fragment(&fragment).build())?
        .uri(api_uri(&broker, "entities")?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
