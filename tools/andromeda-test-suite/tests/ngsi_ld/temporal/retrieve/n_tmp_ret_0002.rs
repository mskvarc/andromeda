//! A deleted scope reads back as a null instance under deleted at.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, scope_deletion::delete_scope};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateOrUpdateTemporalEvolution, RepresentationSpelling},
    },
    model::{
        Lossless,
        NgsiLdMediaType,
        Property,
        RawJson,
        evolution::{ScopeValue, TemporalEntity},
    },
    query::temporal_query::TemporalProperty,
};

/// Pins: a Scope deleted through the Core API reads back under timeproperty=deletedAt as the one Property instance
/// holding the NGSI-LD Null and a deletedAt, normalized and as a single [value, instant] pair in the simplified
/// representation
/// Clause: 4.5.6, 4.5.7, 4.5.9, 4.18, 4.11, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_ret_0002_a_deleted_scope_reads_back_as_a_null_instance_under_deleted_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(80).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    // The model writes a scope as a Property, so the plain list the body carries is written as text.
    let plain_scope: RawJson = r#"["/Madrid/Center"]"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_evolution.render(Lossless::Normalized))?.set_member("scope", &plain_scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution).build())?
        .document(&document);
    let created = broker.send_unchecked(request).await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let deleted = delete_scope(&broker, &vehicle_id).await?;
    expect_that!(deleted.status(), eq(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .time_property(TemporalProperty::DeletedAt)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(&evolution.id, eq(&vehicle_id));
    expect_that!(&evolution.scope, len(eq(1)));
    let instance = evolution.scope.first().or_fail()?;
    expect_that!(instance.value(), eq(&ScopeValue::Removed));
    expect_that!(instance.output().deleted_at, some(anything()));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&vehicle_id)
        .time_property(TemporalProperty::DeletedAt)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let pairs = &retrieved_2.body().scope;
    expect_that!(pairs, len(eq(1)));
    expect_that!(pairs.first().map(|pair| &pair.value), some(eq(&ScopeValue::Removed)));
    Ok(())
}
