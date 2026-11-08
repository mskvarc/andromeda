//! A refused temporal evolution create creates no tenant.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
};

/// Pins: Create or Update Temporal Evolution of an Entity on a new Tenant refused with 400 `BadRequestData` for an
/// `observedAt` with seven fraction digits leaves the Tenant nonexistent: retrieving that Temporal Evolution there
/// answers 404 `NonexistentTenant` echoing the tenant
/// Clause: 5.5.10, 5.6.11, 5.5.4, 6.3.14, Table 6.3.2-1
/// Tenant: private (the subject is creating a Tenant)
/// Recorded: clause 5.5.10 makes implicit Tenant creation an implementation option, created "when an NGSI-LD
/// operation for creating information targets a new Tenant"; Andromeda creates a Tenant only once a creating
/// operation has created information in it, so a refused one creates none
#[gtest]
#[tokio::test]
async fn o_ten_imp_9004_a_refused_temporal_evolution_create_creates_no_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    broker::private_tenant(&broker, &namespace).await?;
    let refused_tenant = broker.with_tenant(format!("{namespace}-refused").parse()?);
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    // The model refuses it (invalid timestamp at speed), so `speed` is written as text.
    let speed: RawJson = r#"[{"type": "Property", "value": 55, "observedAt": "2024-01-01T00:00:00.1234567Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_evolution.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = refused_tenant
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution).build())?
        .document(&document);
    let answer = refused_tenant.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = refused_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(
        &answer_2,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-refused"))
    );
    Ok(())
}
