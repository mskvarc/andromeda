//! An error response echoes the tenant.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless},
};

/// Pins: Create Entity naming a Tenant with a body lacking its id is refused with 400 `BadRequestData` and the
/// response still carries the NGSILD-Tenant header
/// Clause: 6.3.14, 5.5.10, 5.6.1.4
/// Tenant: private (the subject is the NGSILD-Tenant header of a named Tenant)
#[gtest]
#[tokio::test]
async fn n_ten_hdr_0001_an_error_response_echoes_the_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    broker::private_tenant(&broker, &namespace).await?;
    let error_tenant = broker.with_tenant(format!("{namespace}-error").parse()?);
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let placeholder_id = namespace.urn("Placeholder", 1)?;
    let entity = Entity::builder().id(placeholder_id).entity_type(vehicle_type).build()?;
    // The model requires `id`, which the request leaves out, so it is written for a placeholder and removed.
    let document = UncheckedDocument::from_entity(&entity, Lossless::Normalized).remove_member("id");
    let request = error_tenant.unchecked(&CreateEntity::builder().entity(&entity).build())?.document(&document);
    let answer = error_tenant.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-error"))
    );
    Ok(())
}
