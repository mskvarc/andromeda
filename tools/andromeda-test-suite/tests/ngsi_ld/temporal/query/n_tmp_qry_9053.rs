//! Attrs without the scope leaves it out.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{Lossless, NgsiLdMediaType, Property, RawJson, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with attrs=speed answers the Entity without its scope
/// Clause: 4.18, Table 6.18.3.2-1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9053_attrs_without_the_scope_leaves_it_out() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_attrs_no_scope_id = namespace.urn("Vehicle", "attrs-no-scope")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_attrs_no_scope_evolution = TemporalEntity::builder()
        .id(vehicle_attrs_no_scope_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(30).observed_at("2018-08-01T12:03:00Z".parse()?))
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.scope (written {"type":"Property","value":"/Madrid/Centro","observedAt":"2018-08-01T11:00:00Z"})]: {"id":"urn:ngsi-ld:Vehicle:zqns-attrs-no-scope","type":"Vehicle_zqns","scope":[{"type":"Property","value":"/Madrid/Centro","observedAt":"2018-08-01T11:00:00Z"}],"speed":[{"type":"Property","value":30,"observedAt":"2018-08-01T12:03:00Z"}]}), so `scope` is written as text.
    let scope: RawJson = r#"{"type": "Property", "value": "/Madrid/Centro", "observedAt": "2018-08-01T11:00:00Z"}"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_attrs_no_scope_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_attrs_no_scope_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_attrs_no_scope_id.clone()])
        .attrs(vec!["speed".parse()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let item = page.items().first().or_fail()?;
    expect_that!(item.scope, is_empty());
    expect_that!(page.items(), nth(0, has_evolution("speed", len(eq(1)))));
    Ok(())
}
