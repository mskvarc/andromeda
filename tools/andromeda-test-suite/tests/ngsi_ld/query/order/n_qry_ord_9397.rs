//! orderBy on a dataset id, value type or object type member orders by its text.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, Relationship, VocabTerm},
    query::{Locality, ordering::OrderBy},
};

/// Pins: Query Entities with `orderBy=reading.datasetId`, `orderBy=reading.valueType` and `orderBy=feeds.objectType`
/// answers 200 with the entities ordered by the dataset URI or the expanded type IRI, and last, in both directions,
/// the entity whose `reading` is a default instance, declares no valueType, or whose `feeds` declares two types
/// Clause: 4.5.2, 4.5.3, 4.5.5, 4.23.1, 4.23.2, 4.23.3
#[gtest]
#[tokio::test]
async fn n_qry_ord_9397_order_by_on_a_dataset_value_type_or_object_type_member_orders_by_its_text() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let member_order_type = namespace.entity_type("MemberOrder")?;
    let [a, b, c, d] = ["a", "b", "c", "d"].map(|suffix| namespace.urn("Sensor", suffix));
    let (a, b, c, d) = (a?, b?, c?, d?);
    let target_id = namespace.urn("Device", 1)?;
    let [first_type, second_type, third_type] = ["First", "Second", "Third"].map(|base| namespace.entity_type(base));
    let (first_type, second_type, third_type) = (first_type?, second_type?, third_type?);
    // Identifier order differs from the expected order in both directions.
    for (id, dataset, value_type, object_types) in [
        (&a, Some("d2"), Some(&second_type), vec![VocabTerm::from(&second_type)]),
        (&b, Some("d3"), Some(&third_type), vec![VocabTerm::from(&third_type)]),
        (&c, Some("d1"), Some(&first_type), vec![VocabTerm::from(&first_type)]),
        (&d, None, None, vec![VocabTerm::from(&first_type), VocabTerm::from(&second_type)]),
    ] {
        let dataset = dataset.map(|suffix| namespace.dataset_id("Dataset", suffix)).transpose()?;
        let entity = Entity::builder()
            .id(id.clone())
            .entity_type(member_order_type.clone())
            .property(
                "reading",
                Property::builder(1).maybe_dataset_id(dataset).maybe_value_type(value_type.map(VocabTerm::from)),
            )
            .relationship("feeds", Relationship::builder(target_id.clone()).object_type(object_types))
            .build()?;
        broker.entities().create(&entity).send().await?.done()?;
    }
    // Clause 4.23.2: "Attribute does not exist" ranks last, also when the term runs descending.
    for (order_by, expected) in [
        ("reading.datasetId", [&c, &a, &b, &d]),
        ("reading.datasetId;desc", [&b, &a, &c, &d]),
        ("reading.valueType", [&c, &a, &b, &d]),
        ("reading.valueType;desc", [&b, &a, &c, &d]),
        ("feeds.objectType", [&c, &a, &b, &d]),
        ("feeds.objectType;desc", [&b, &a, &c, &d]),
    ] {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&member_order_type)
            .locality(Locality::Local)
            .ordering(order_by.parse::<OrderBy>()?)
            .send()
            .await?;
        expect_that!(page.items(), ids_in_order(&expected.map(Clone::clone)), "orderBy {order_by}");
    }
    Ok(())
}
