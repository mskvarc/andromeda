//! A batch item whose id is a context alias of at id is created.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    core_context::core_context,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    unchecked_outcome::read_as,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{Entity, Lossless, NgsiLdMediaType, OneOrMany, RawJson},
};

/// Pins: Batch Entity Creation of one item writing its id under a term its own @context maps to @id, beside one
/// writing id, answers 201 with both ids, and both Entities read back
/// Clause: 4.4, 5.5.7, 5.6.7
#[gtest]
#[tokio::test]
async fn n_bat_cre_9337_a_batch_item_whose_id_is_a_context_alias_of_at_id_is_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let building_batch_plain_id = namespace.urn("Building", "batch-plain")?;
    let building_type = namespace.entity_type("Building")?;
    let building_batch_eid_id = namespace.urn("Building", "batch-eid")?;
    let entities = [
        Entity::builder()
            .id(building_batch_plain_id.clone())
            .entity_type(building_type.clone())
            .property("height", 1)
            .build()?,
        Entity::builder()
            .id(building_batch_eid_id.clone())
            .entity_type(building_type.clone())
            .property("height", 2)
            .build()?,
    ];
    // JSON-LD 1.1 section 4.1.3: a term the item's own `@context` maps to `@id` names the entity's id.
    let aliased: RawJson = r#"{"eid": "@id"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&entities[1], Lossless::Normalized)
        .rename_member("id", "eid")
        .set_member("@context", &aliased);
    let create = BatchCreate::builder().entities(&entities).build();
    let request = core_context_body
        .unchecked(&create)?
        .documents(&[UncheckedDocument::from_entity(&entities[0], Lossless::Normalized), document]);
    let sent = request.as_request().clone();
    let answer = core_context_body.send_unchecked(request).await?;
    let created = read_as(&create, &sent, answer)?.done()?;
    expect_that!(created, unordered_elements_are![eq(&building_batch_plain_id), eq(&building_batch_eid_id)]);
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json.entities().retrieve(&building_batch_eid_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_batch_eid_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type)));
    expect_that!(entity, has_attribute("height", property().value(2)));
    expect_that!(entity, lacks_attribute("eid"));
    let retrieved_2 = json.entities().retrieve(&building_batch_plain_id).send().await?;
    expect_that!(retrieved_2.body(), has_attribute("height", property().value(1)));
    Ok(())
}
