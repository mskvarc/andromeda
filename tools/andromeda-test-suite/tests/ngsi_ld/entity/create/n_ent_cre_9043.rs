//! Creates sent at once sharing never-used attribute names are all created.

use andromeda_test_suite::{answered_status::AnsweredStatus, attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace};
use futures::future::join_all;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// The Create Entity requests sent at once.
const PARALLEL_CREATES: u32 = 8;

/// The attribute IRIs every one of them carries, none of which any Entity has used.
const NEVER_USED_ATTRIBUTES: u32 = 5;

/// Pins: eight Create Entity requests sent at once, each carrying the same five attribute IRIs no Entity has used,
/// each answer 201, and each Entity reads back with all five and the values it was created with
/// Clause: 5.6.1, 5.5.4, 4.5.2
#[gtest]
#[tokio::test]
async fn n_ent_cre_9043_creates_sent_at_once_sharing_never_used_attribute_names_are_all_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let widget_type = namespace.entity_type("Widget")?;
    let attributes: Vec<String> = (0..NEVER_USED_ATTRIBUTES)
        .map(|index| format!("https://example.test/cold/{namespace}-{index}"))
        .collect();
    let mut widgets = Vec::new();
    for index in 0..PARALLEL_CREATES {
        let mut widget = Entity::builder().id(namespace.urn("Widget", index)?).entity_type(widget_type.clone());
        for (value, attribute) in (0_u32..).zip(&attributes) {
            widget = widget.property(attribute.as_str(), value);
        }
        widgets.push(widget.build()?);
    }
    let widgets = widgets;
    let responding = broker.with_responses();
    let entities = responding.entities();
    let answers = join_all(widgets.iter().map(|widget| entities.create(widget).send())).await;
    expect_that!(
        answers.iter().map(AnsweredStatus::answered_status).collect::<Vec<_>>(),
        each(some(eq(&StatusCode::CREATED)))
    );
    for widget in &widgets {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&widget.id)
            .send()
            .await?;
        for (value, attribute) in (0_u32..).zip(&attributes) {
            expect_that!(retrieved.body(), has_attribute(attribute, any_attribute().value(value)));
        }
    }
    Ok(())
}
