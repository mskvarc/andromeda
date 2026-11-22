//! A scope ending in the multi-level wildcard inside a group selects its whole hierarchy.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    raw_call::target,
    unchecked_call::exchange,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::Entity;

/// Pins: Query Entities with `scopeQ` `(/Madrid/#;/CompanyA)` selects exactly the Entities holding `/Madrid` or a
/// Scope below it and also `/CompanyA`, and `(/Madrid/#;/CompanyA)|/CompanyB` adds exactly the one holding `/CompanyB`
/// Clause: 4.19, 5.7.2
/// Recorded: clause 4.19 adds the `#` wildcard "at the end of a Scope specification", and a Scope inside a group is
/// one, though its grammar writes the group without the `/#`
#[gtest]
#[tokio::test]
async fn n_qry_scp_0008_a_scope_ending_in_the_multi_level_wildcard_inside_a_group_selects_its_whole_hierarchy() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let below_id = namespace.urn("Building", "a")?;
    let anchor_id = namespace.urn("Building", "b")?;
    let other_company_id = namespace.urn("Building", "e")?;
    let entities_written = [
        Entity::builder()
            .id(below_id.clone())
            .entity_type(building_type.clone())
            .scope(["/Madrid/Gardens/ParqueNorte", "/CompanyA"])
            .build()?,
        // Clause 4.19 EXAMPLE 2: the trailing # matches the given scope itself as well as the hierarchy below it.
        Entity::builder()
            .id(anchor_id.clone())
            .entity_type(building_type.clone())
            .scope(["/Madrid", "/CompanyA"])
            .build()?,
        Entity::builder()
            .id(namespace.urn("Building", "c")?)
            .entity_type(building_type.clone())
            .scope("/Madrid/Gardens")
            .build()?,
        Entity::builder()
            .id(namespace.urn("Building", "d")?)
            .entity_type(building_type.clone())
            .scope("/CompanyA")
            .build()?,
        Entity::builder()
            .id(other_company_id.clone())
            .entity_type(building_type.clone())
            .scope(["/Madrid/Gardens", "/CompanyB"])
            .build()?,
        Entity::builder()
            .id(namespace.urn("Building", "f")?)
            .entity_type(building_type.clone())
            .scope(["/Barcelona/Madrid", "/CompanyA"])
            .build()?,
    ];
    broker.entities().batch_create(&entities_written).send().await?.done()?;
    let type_text = building_type.to_string();
    // The client cannot write a parameter of this read, so it is sent as written.
    let read = target("entities", &[("type", type_text.as_str()), ("scopeQ", "(/Madrid/#;/CompanyA)")]);
    let answer = broker
        .send_unchecked(exchange(&broker, Method::GET, &read, &[("Accept", "application/json")], None)?)
        .await;
    expect_that!(&answer, answered(StatusCode::OK));
    let selected = entities(&answer?)?;
    expect_that!(selected, len(eq(2)));
    expect_that!(selected, contains(has_id(&below_id)));
    expect_that!(selected, contains(has_id(&anchor_id)));
    // The client cannot write a parameter of this read, so it is sent as written.
    let read_2 = target("entities", &[("type", type_text.as_str()), ("scopeQ", "(/Madrid/#;/CompanyA)|/CompanyB")]);
    let answer_2 = broker
        .send_unchecked(exchange(&broker, Method::GET, &read_2, &[("Accept", "application/json")], None)?)
        .await;
    expect_that!(&answer_2, answered(StatusCode::OK));
    let selected_2 = entities(&answer_2?)?;
    expect_that!(selected_2, len(eq(3)));
    expect_that!(selected_2, contains(has_id(&below_id)));
    expect_that!(selected_2, contains(has_id(&anchor_id)));
    expect_that!(selected_2, contains(has_id(&other_company_id)));
    Ok(())
}
