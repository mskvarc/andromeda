//! An Entity whose `objectType` and `vocab` the release stored though they expand to no IRI reads back as written once
//! the hub is upgraded.

use andromeda_test_suite::{
    body_json::json_equals,
    broker::{self, BrokerClient},
    broker_process::BrokerProcess,
    config::BrokerRole,
    example_prefix::{UNEXPANDABLE_VALUES, under_example_prefix},
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateEntity, RetrieveEntity},
    },
    model::{Attribute, Entity, EntityId, Input, Lossless, NgsiLdMediaType, RawJson, Relationship, VocabProperty, VocabTerm},
};

/// `attribute` with its `member` set to `value`, which the model refuses to hold.
fn with_member_set(attribute: &Attribute<Input>, member: &str, value: &str) -> Result<RawJson> {
    Ok(UncheckedDocument::try_from(attribute.render(Lossless::Normalized))?
        .set_text(member, value)
        .to_string()
        .parse()?)
}

/// Expects each Building of `buildings` to be retrieved by `client`, under the core @context alone, with the
/// `objectType` of its owner and the `vocab` of its category as the value it was created with, read as received since
/// the model refuses to hold the value.
async fn expect_read_back_as_written(client: &BrokerClient, buildings: &[(EntityId, &str)], hub: &str) -> Result<()> {
    let json = client.with_accept(Accept::new(NgsiLdMediaType::Json));
    for (id, value) in buildings {
        let retrieved = json
            .send_unchecked(json.unchecked(&RetrieveEntity::builder().id(id).format(Lossless::Normalized).build())?)
            .await?;
        expect_that!(retrieved.body(), json_equals("$.owner.objectType", *value), "{hub} {value}");
        expect_that!(retrieved.body(), json_equals("$.category.vocab", *value), "{hub} {value}");
    }
    Ok(())
}

/// Pins: a Building the hub created on v1.0.0 under an @context holding the prefix ex, whose Relationship owner has the
/// objectType and whose `VocabProperty` category has the vocab `ex:a{b`, `ex:%`, `urn:x:{` or `https://example.org/{`,
/// which v1.0.0 answers 201 and retrieves under the core @context with the value as written, is retrieved with the same
/// objectType and vocab once the hub runs the working tree
/// Clause: 4.5.3.2, 4.6.2, 5.7.1
/// Recorded: the working tree refuses a type or vocabulary term its @context expands to no absolute IRI, and reads a
/// value the release stored before as the release wrote it, since a stored name is read through the grammar it was
/// stored under
#[gtest]
#[tokio::test]
async fn a_upg_tnm_0001_an_object_type_or_vocab_the_release_stored_that_expands_to_no_iri_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let hub = broker::client(BrokerRole::TypesHub)?;
    let writer = under_example_prefix(&hub)?;
    let placeholder: VocabTerm = "ex:Placeholder".parse()?;
    let owner = Attribute::from(Relationship::builder(namespace.urn("Person", "o")?).object_type(placeholder.clone()).build()?);
    let category = Attribute::from(VocabProperty::builder(placeholder).build()?);
    let mut buildings = Vec::new();
    for (position, value) in UNEXPANDABLE_VALUES.into_iter().enumerate() {
        let building_id = namespace.urn("Building", position)?;
        let building = Entity::builder()
            .id(building_id.clone())
            .entity_type(namespace.entity_type("Building")?)
            .build()?;
        let document = UncheckedDocument::from_entity(&building, Lossless::Normalized)
            .set_member("owner", &with_member_set(&owner, "objectType", value)?)
            .set_member("category", &with_member_set(&category, "vocab", value)?);
        let request = writer.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
        expect_that!(&writer.send_unchecked(request).await, answered(StatusCode::CREATED), "{value}");
        buildings.push((building_id, value));
    }
    expect_read_back_as_written(&hub, &buildings, "release").await?;

    BrokerProcess::of(BrokerRole::TypesHub)?.upgrade().await?;
    expect_read_back_as_written(&hub, &buildings, "upgraded").await?;
    Ok(())
}
