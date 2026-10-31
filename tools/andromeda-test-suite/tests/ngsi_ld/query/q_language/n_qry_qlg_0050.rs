//! A selective pattern past the first two hundred entities pages in identifier order.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, Input, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities with q label~="code-02`[5-9]``[0-9]`" over 300 entities whose fifty matches all sort after
/// the first 200 answers them in identifier order, page by page, with their total
/// Clause: 4.9, 5.5.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0050_a_selective_pattern_past_the_first_two_hundred_entities_pages_in_identifier_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sign_type = namespace.entity_type("Sign")?;
    let entities = (1..=300)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Sign", format!("{n:04}"))?)
                .entity_type(sign_type.clone())
                .property("label", format!("code-{n:04}"))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sign_type)
        .q(q::attr("label").matches("\"code-02[5-9][0-9]\""))
        .limit(20)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(50)));
    let expected_ids = (250..=269).map(|n| namespace.urn("Sign", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page.items(), ids_in_order(&expected_ids));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sign_type)
        .q(q::attr("label").matches("\"code-02[5-9][0-9]\""))
        .limit(20)
        .offset(40)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(50)));
    let expected_ids_2 = (290..=299).map(|n| namespace.urn("Sign", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_2.items(), ids_in_order(&expected_ids_2));
    Ok(())
}
