//! A batch create refusing every entity creates no tenant.

use andromeda_test_suite::{broker, namespace::Namespace, partial_outcome::reported, problem::problem};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Creation on a new Tenant in which every entity is refused answers 207 with no success and an
/// error for each entity, and leaves the Tenant nonexistent: a following query there answers 404
/// `NonexistentTenant` echoing the tenant
/// Clause: 5.5.10, 5.6.7.4, 5.5.4, 6.3.14, Table 6.3.2-1
/// Tenant: private (the subject is creating a Tenant)
/// Recorded: clause 5.5.10 makes implicit Tenant creation an implementation option, created "when an NGSI-LD
/// operation for creating information targets a new Tenant"; Andromeda creates a Tenant only once a creating
/// operation has created information in it, so a refused one creates none
#[gtest]
#[tokio::test]
async fn o_ten_imp_9002_a_batch_create_refusing_every_entity_creates_no_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    broker::private_tenant(&broker, &namespace).await?;
    let refused_tenant = broker.with_tenant(format!("{namespace}-refused").parse()?);
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle = Entity::builder().id(vehicle_id).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid value at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": null}"#.parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("speed", &speed);
    let vehicle_2 = Entity::builder().id(vehicle_2_id).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid value at speed), so `speed` is written as text.
    let speed_2: RawJson = r#"{"type": "Property", "value": null}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&vehicle_2, Lossless::Normalized).set_member("speed", &speed_2);
    let entities = [vehicle, vehicle_2];
    let request = refused_tenant
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[document, document_2]);
    let answer = refused_tenant.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(0)));
    expect_that!(&report.errors, len(eq(2)));
    let answer_2 = refused_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_2,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-refused"))
    );
    Ok(())
}
