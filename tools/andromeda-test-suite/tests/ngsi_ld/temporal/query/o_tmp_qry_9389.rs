//! A Scope set inside the window replaces the one in force at its start exactly once.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{
        DateTime,
        Lossless,
        NgsiLdMediaType,
        OneOrMany,
        Property,
        RawJson,
        Scope,
        evolution::{ScopeInstance, ScopeValue, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities between 12:00 and 13:00 of a Vehicle whose scope /<ns>/A is observed at
/// 11:00 and whose scope /<ns>/B, added afterwards, is observed at 12:30 answers two scope instances: /<ns>/A, in
/// force when the window opens, then /<ns>/B
/// Clause: 4.18, 4.5.6, 5.7.4.4, Annex C.5.16
/// Recorded: clauses 5.6.11.4 and 5.6.12.4 do not say what a scope member of a Temporal Evolution Fragment records;
/// Andromeda records a reified scope instance as given, dated by its own `observedAt`
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9389_a_scope_set_inside_the_window_replaces_the_one_in_force_at_its_start_exactly_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "rescoped")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(30).observed_at("2018-08-01T12:10:00Z".parse()?))
        .build()?;
    // The model refuses a reified scope on the builder, so `scope` is written as text.
    let scope: RawJson = format!(r#"{{"type": "Property", "value": "/{namespace}/A", "observedAt": "2018-08-01T11:00:00Z"}}"#).parse()?;
    let document = UncheckedDocument::try_from(evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build())?
        .document(&document);
    let created = broker.send_unchecked(request).await;
    expect_that!(&created, answered(StatusCode::CREATED));
    // The scope is a single instance object, which Add Attributes to Temporal Evolution does not write, so the body is sent as written.
    let appended = broker
        .send_unchecked(json_request_to(
            &broker,
            Method::POST,
            &format!("temporal/entities/{vehicle_id}/attrs"),
            &format!(r#"{{"scope": {{"type": "Property", "value": "/{namespace}/B", "observedAt": "2018-08-01T12:30:00Z"}}}}"#),
        )?)
        .await;
    expect_that!(&appended, answered(StatusCode::NO_CONTENT));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let item = page.items().first().or_fail()?;
    let first = ScopeValue::Scopes(OneOrMany::one(format!("/{namespace}/A").parse::<Scope>()?));
    let second = ScopeValue::Scopes(OneOrMany::one(format!("/{namespace}/B").parse::<Scope>()?));
    expect_that!(item.scope.iter().map(ScopeInstance::value).collect::<Vec<_>>(), eq(&vec![&first, &second]));
    expect_that!(
        item.scope.iter().map(|instance| instance.observed_at().copied()).collect::<Vec<_>>(),
        eq(&vec![
            Some("2018-08-01T11:00:00Z".parse::<DateTime>()?),
            Some("2018-08-01T12:30:00Z".parse::<DateTime>()?)
        ])
    );
    Ok(())
}
