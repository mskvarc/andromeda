//! A null fragment deletes the instance its dataset id names.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{
        AttributeKind,
        AttributeName,
        Change,
        Entity,
        GeoProperty,
        InstanceDeletion,
        JsonProperty,
        LanguageMap,
        LanguageProperty,
        ListProperty,
        Lossless,
        NgsiLdMediaType,
        Property,
        RawJson,
        Relationship,
        Value,
        VocabProperty,
        VocabTerm,
        geometry::{Position, Shape},
    },
};

/// Pins: Partial Attribute Update with the deletion null of each attribute family and the `datasetId` of one of two
/// instances answers 204, deletes that instance and leaves the other rendered as a single attribute
/// Clause: 5.5.8, 5.6.4.4, 5.6.5, 4.5.0, 4.5.5
#[gtest]
#[tokio::test]
async fn n_att_pat_9013_a_null_fragment_deletes_the_instance_its_dataset_id_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_instance_property_id = namespace.urn("Building", "instance-Property")?;
    let building_type = namespace.entity_type("Building")?;
    let dataset_01 = namespace.dataset_id("Dataset", "01")?;
    let dataset_02 = namespace.dataset_id("Dataset", "02")?;
    let building_instance_relationship_id = namespace.urn("Building", "instance-Relationship")?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let building_instance_geo_property_id = namespace.urn("Building", "instance-GeoProperty")?;
    let building_instance_language_property_id = namespace.urn("Building", "instance-LanguageProperty")?;
    let building_instance_json_property_id = namespace.urn("Building", "instance-JsonProperty")?;
    let building_instance_vocab_property_id = namespace.urn("Building", "instance-VocabProperty")?;
    let building_instance_list_property_id = namespace.urn("Building", "instance-ListProperty")?;
    let building_instance_list_relationship_id = namespace.urn("Building", "instance-ListRelationship")?;
    let building_instance_property = Entity::builder()
        .id(building_instance_property_id.clone())
        .entity_type(building_type.clone())
        .property("street", Property::builder(4).dataset_id(namespace.dataset_id("Dataset", "01")?))
        .property("street", Property::builder(4).dataset_id(namespace.dataset_id("Dataset", "02")?))
        .build()?;
    broker.entities().create(&building_instance_property).send().await?.done()?;
    let deletion = Change::Delete(InstanceDeletion::new(AttributeKind::Property).with_dataset_id(dataset_01.clone()));
    broker
        .entities()
        .update_attribute(&building_instance_property_id, &"street".parse::<AttributeName>()?, deletion)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_instance_property_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("street", 1));
    expect_that!(entity, has_attribute("street", any_attribute().with_dataset_id(dataset_02.clone())));
    let building_instance_relationship = Entity::builder()
        .id(building_instance_relationship_id.clone())
        .entity_type(building_type.clone())
        .relationship(
            "street",
            Relationship::builder(sensor_id.clone()).dataset_id(namespace.dataset_id("Dataset", "01")?),
        )
        .relationship(
            "street",
            Relationship::builder(sensor_id.clone()).dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .build()?;
    broker.entities().create(&building_instance_relationship).send().await?.done()?;
    let deletion_2 = Change::Delete(InstanceDeletion::new(AttributeKind::Relationship).with_dataset_id(dataset_01.clone()));
    broker
        .entities()
        .update_attribute(&building_instance_relationship_id, &"street".parse::<AttributeName>()?, deletion_2)
        .send()
        .await?
        .done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_instance_relationship_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_instances("street", 1));
    expect_that!(entity_2, has_attribute("street", any_attribute().with_dataset_id(dataset_02.clone())));
    let building_instance_geo_property = Entity::builder()
        .id(building_instance_geo_property_id.clone())
        .entity_type(building_type.clone())
        .geo_property(
            "street",
            GeoProperty::builder(Shape::Point(Position::try_new(2.2944, 48.8585)?)).dataset_id(namespace.dataset_id("Dataset", "01")?),
        )
        .geo_property(
            "street",
            GeoProperty::builder(Shape::Point(Position::try_new(2.2944, 48.8585)?)).dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .build()?;
    broker.entities().create(&building_instance_geo_property).send().await?.done()?;
    let deletion_3 = Change::Delete(InstanceDeletion::new(AttributeKind::GeoProperty).with_dataset_id(dataset_01.clone()));
    broker
        .entities()
        .update_attribute(&building_instance_geo_property_id, &"street".parse::<AttributeName>()?, deletion_3)
        .send()
        .await?
        .done()?;
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_instance_geo_property_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, has_instances("street", 1));
    expect_that!(entity_3, has_attribute("street", any_attribute().with_dataset_id(dataset_02.clone())));
    let building_instance_language_property = Entity::builder()
        .id(building_instance_language_property_id.clone())
        .entity_type(building_type.clone())
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Avenue"), ("en", "Avenue")])?).dataset_id(namespace.dataset_id("Dataset", "01")?),
        )
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Avenue"), ("en", "Avenue")])?).dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .build()?;
    broker.entities().create(&building_instance_language_property).send().await?.done()?;
    let deletion_4 = Change::Delete(InstanceDeletion::new(AttributeKind::LanguageProperty).with_dataset_id(dataset_01.clone()));
    broker
        .entities()
        .update_attribute(&building_instance_language_property_id, &"street".parse::<AttributeName>()?, deletion_4)
        .send()
        .await?
        .done()?;
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_instance_language_property_id)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(entity_4, has_instances("street", 1));
    expect_that!(entity_4, has_attribute("street", any_attribute().with_dataset_id(dataset_02.clone())));
    let building_instance_json_property = Entity::builder()
        .id(building_instance_json_property_id.clone())
        .entity_type(building_type.clone())
        .json_property(
            "street",
            JsonProperty::builder(r#"{"id":1}"#.parse::<RawJson>()?).dataset_id(namespace.dataset_id("Dataset", "01")?),
        )
        .json_property(
            "street",
            JsonProperty::builder(r#"{"id":1}"#.parse::<RawJson>()?).dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .build()?;
    broker.entities().create(&building_instance_json_property).send().await?.done()?;
    let deletion_5 = Change::Delete(InstanceDeletion::new(AttributeKind::JsonProperty).with_dataset_id(dataset_01.clone()));
    broker
        .entities()
        .update_attribute(&building_instance_json_property_id, &"street".parse::<AttributeName>()?, deletion_5)
        .send()
        .await?
        .done()?;
    let retrieved_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_instance_json_property_id)
        .send()
        .await?;
    let entity_5 = retrieved_5.body();
    expect_that!(entity_5, has_instances("street", 1));
    expect_that!(entity_5, has_attribute("street", any_attribute().with_dataset_id(dataset_02.clone())));
    let building_instance_vocab_property = Entity::builder()
        .id(building_instance_vocab_property_id.clone())
        .entity_type(building_type.clone())
        .vocab_property(
            "street",
            VocabProperty::builder("Sensor".parse::<VocabTerm>()?).dataset_id(namespace.dataset_id("Dataset", "01")?),
        )
        .vocab_property(
            "street",
            VocabProperty::builder("Sensor".parse::<VocabTerm>()?).dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .build()?;
    broker.entities().create(&building_instance_vocab_property).send().await?.done()?;
    let deletion_6 = Change::Delete(InstanceDeletion::new(AttributeKind::VocabProperty).with_dataset_id(dataset_01.clone()));
    broker
        .entities()
        .update_attribute(&building_instance_vocab_property_id, &"street".parse::<AttributeName>()?, deletion_6)
        .send()
        .await?
        .done()?;
    let retrieved_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_instance_vocab_property_id)
        .send()
        .await?;
    let entity_6 = retrieved_6.body();
    expect_that!(entity_6, has_instances("street", 1));
    expect_that!(entity_6, has_attribute("street", any_attribute().with_dataset_id(dataset_02.clone())));
    let building_instance_list_property = Entity::builder()
        .id(building_instance_list_property_id.clone())
        .entity_type(building_type.clone())
        .list_property(
            "street",
            ListProperty::builder(vec![Value::from(1), Value::from(2)]).dataset_id(namespace.dataset_id("Dataset", "01")?),
        )
        .list_property(
            "street",
            ListProperty::builder(vec![Value::from(1), Value::from(2)]).dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .build()?;
    broker.entities().create(&building_instance_list_property).send().await?.done()?;
    let deletion_7 = Change::Delete(InstanceDeletion::new(AttributeKind::ListProperty).with_dataset_id(dataset_01.clone()));
    broker
        .entities()
        .update_attribute(&building_instance_list_property_id, &"street".parse::<AttributeName>()?, deletion_7)
        .send()
        .await?
        .done()?;
    let retrieved_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_instance_list_property_id)
        .send()
        .await?;
    let entity_7 = retrieved_7.body();
    expect_that!(entity_7, has_instances("street", 1));
    expect_that!(entity_7, has_attribute("street", any_attribute().with_dataset_id(dataset_02.clone())));
    let building_instance_list_relationship = Entity::builder()
        .id(building_instance_list_relationship_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.street[0].objectList[0] (written "urn:ngsi-ld:Sensor:zqns-1")]: {"id":"urn:ngsi-ld:Building:zqns-instance-ListRelationship","type":"Building_zqns","street":[{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Sensor:zqns-1"}],"datasetId":"urn:ngsi-ld:Dataset:zqns-01"},{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Sensor:zqns-1"}],"datasetId":"urn:ngsi-ld:Dataset:zqns-02"}]}), so `street` is written as text.
    let street: RawJson = format!(r#"[{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Sensor:{namespace}-1"], "datasetId": "urn:ngsi-ld:Dataset:{namespace}-01"}}, {{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Sensor:{namespace}-1"], "datasetId": "urn:ngsi-ld:Dataset:{namespace}-02"}}]"#).parse()?;
    let document = UncheckedDocument::from_entity(&building_instance_list_relationship, Lossless::Normalized).set_member("street", &street);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_instance_list_relationship).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let deletion_8 = Change::Delete(InstanceDeletion::new(AttributeKind::ListRelationship).with_dataset_id(dataset_01.clone()));
    broker
        .entities()
        .update_attribute(&building_instance_list_relationship_id, &"street".parse::<AttributeName>()?, deletion_8)
        .send()
        .await?
        .done()?;
    let retrieved_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_instance_list_relationship_id)
        .send()
        .await?;
    let entity_8 = retrieved_8.body();
    expect_that!(entity_8, has_instances("street", 1));
    expect_that!(entity_8, has_attribute("street", any_attribute().with_dataset_id(dataset_02.clone())));
    Ok(())
}
