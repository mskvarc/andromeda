//! A batch create with a refused entity still creates the tenant.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    partial_outcome::reported,
    problem::{answered, problem},
    report_matcher::batch_error,
};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::{
        BatchOperationResult,
        UncheckedDocument,
        headers::{Accept, NgsiLdTenant},
        operation::BatchCreate,
    },
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson, Tenant},
};

/// Pins: Batch Entity Creation on a new Tenant in which one entity is refused answers 207 echoing the tenant,
/// creates the Tenant with the accepted entity, and leaves the refused one absent
/// Clause: 5.5.10, 5.6.7.4, 6.3.14, 5.5.4
/// Tenant: private (the subject is creating a Tenant)
/// Recorded: clause 5.5.10 makes implicit Tenant creation an implementation option and says the other operations on
/// a non-existing Tenant "should" raise `NonexistentTenant`; Andromeda creates Tenants implicitly, exactly on the
/// six operations the clause lists
#[gtest]
#[tokio::test]
async fn o_ten_imp_0003_a_batch_create_with_a_refused_entity_still_creates_the_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    broker::private_tenant(&broker, &namespace).await?;
    let new_tenant = broker.with_tenant(format!("{namespace}-new").parse()?);
    let vehicle_created_id = namespace.urn("Vehicle", "created")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_refused_id = namespace.urn("Vehicle", "refused")?;
    // A null Property value is refused for its entity alone (clause 5.5.4).
    let vehicle_created = Entity::builder()
        .id(vehicle_created_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", "created")
        .build()?;
    let document = UncheckedDocument::from_entity(&vehicle_created, Lossless::Normalized);
    let vehicle_refused = Entity::builder().id(vehicle_refused_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid value at name), so `name` is written as text.
    let name: RawJson = r#"{"type": "Property", "value": null}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&vehicle_refused, Lossless::Normalized).set_member("name", &name);
    let entities = [vehicle_created, vehicle_refused];
    let request = new_tenant
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[document, document_2]);
    let answer = new_tenant.send_unchecked(request).await;
    expect_that!(
        &answer,
        answered(StatusCode::MULTI_STATUS).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-new"))
    );
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&vehicle_created_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&vehicle_refused_id)));
    let headed = new_tenant
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_created_id)
        .send()
        .await?;
    expect_that!(
        headed.head.typed::<NgsiLdTenant>().valid().map(NgsiLdTenant::tenant),
        some(eq(&format!("{namespace}-new").parse::<Tenant>()?))
    );
    let retrieved = &headed.outcome;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("created")));
    let answer_2 = new_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_refused_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
