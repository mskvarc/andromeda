//! The multi line option keeps a dot matching a newline.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: q name~="(?im)^prvi.čebela$" selects a value holding prvi, a newline and Čebela: (?m) makes ^ and $ line
/// anchors and leaves . matching the newline
/// Clause: 4.9, 5.7.2.4
/// Recorded: clause 4.9 hands `RegExp` to IEEE 1003.2, which has no inline options and no shorthand classes;
/// Andromeda admits the case-insensitive option and the shorthand classes \d, \w and \s as extensions, folding case
/// by Unicode simple case folding and classifying by Unicode; (?m) makes ^ and $ line anchors and, as IEEE 1003.2
/// reads ., leaves . matching a newline
#[gtest]
#[tokio::test]
async fn a_qry_qlg_0045_the_multi_line_option_keeps_a_dot_matching_a_newline() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let line_id = namespace.urn("Line", 1)?;
    let line_type = namespace.entity_type("Line")?;
    let line_2_id = namespace.urn("Line", 2)?;
    let entities = [
        Entity::builder()
            .id(line_id.clone())
            .entity_type(line_type.clone())
            .property("name", "prvi\nČebela")
            .build()?,
        Entity::builder()
            .id(line_2_id)
            .entity_type(line_type.clone())
            .property("name", "drugi\nČebela")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&line_type)
        .q(q::attr("name").matches("\"(?im)^prvi.čebela$\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&line_id)));
    Ok(())
}
