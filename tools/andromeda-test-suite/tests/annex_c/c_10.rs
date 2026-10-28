//! Annex C.10: a Property whose `valueType` names the datatype of its value, a string address beside a
//! `PostalAddress` object (clause 4.5.2.2).

use crate::{
    printed_document::expect_with_any_context,
    served_context::{ServedContexts, linked, retrieved_ld},
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use indexmap::IndexMap;
use ngsi_ld::model::{Entity, Input, Property, Value, VocabTerm};
use serde_json::json;
use url::Url;

/// Serves the example's `@context`, the Place type carrying the namespace.
async fn place_context(served: &ServedContexts, namespace: &Namespace) -> Result<Url> {
    let place = namespace.suffixed("Place");
    let document = json!({"@context": {
        "example": "http://example.org/",
        "xsd": "http://www.w3.org/2001/XMLSchema#",
        "address": "example:address",
        "city": "example:city",
        "country": "example:country",
        "street": "example:street",
        place.as_str(): format!("example:{place}"),
        "PostalAddress": "example:PostalAddress",
        "String": "xsd:String",
    }});
    served.serve("place-context.jsonld", &document).await
}

/// The Place `27182`, its address the string `Pariser Platz, Berlin, Germany` of valueType `String`.
fn string_place(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Place", 27182)?)
        .entity_type(namespace.entity_type("Place")?)
        .property(
            "address",
            Property::builder("Pariser Platz, Berlin, Germany").value_type("String".parse::<VocabTerm>()?),
        )
        .build()?)
}

/// The Place `31415`, its address a street, city and country object of valueType `PostalAddress`.
fn postal_place(namespace: &Namespace) -> Result<Entity<Input>> {
    let address = Value::Object(IndexMap::from([
        ("street".to_owned(), Value::from("Straße des 17. Juni")),
        ("city".to_owned(), Value::from("Berlin")),
        ("country".to_owned(), Value::from("Germany")),
    ]));
    Ok(Entity::builder()
        .id(namespace.urn("Place", 31415)?)
        .entity_type(namespace.entity_type("Place")?)
        .property("address", Property::builder(address).value_type("PostalAddress".parse::<VocabTerm>()?))
        .build()?)
}

/// Pins: The Place whose address is a string with valueType String is retrieved exactly as C.10 prints it
/// Clause: C.10, 4.5.2.2, 5.7.1
#[gtest]
#[tokio::test]
async fn c_10_0001_a_string_address_keeps_its_value_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &place_context(&served, &namespace).await?)?;
    let string = string_place(&namespace)?;
    broker.entities().create(&string).send().await?;
    broker.entities().create(&postal_place(&namespace)?).send().await?;
    let body = retrieved_ld(&broker, &string.id).await?;
    expect_with_any_context(
        body,
        &json!({
            "id": string.id.to_string(),
            "type": namespace.suffixed("Place"),
            "address": {"type": "Property", "value": "Pariser Platz, Berlin, Germany", "valueType": "String"},
        }),
    );
    served.stop();
    Ok(())
}

/// Pins: The Place whose address is a `PostalAddress` object is retrieved exactly as C.10 prints it beside the String
/// one, each with its own valueType
/// Clause: C.10, 4.5.2.2, 5.7.1
#[gtest]
#[tokio::test]
async fn c_10_0002_two_value_shapes_stay_distinguishable() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &place_context(&served, &namespace).await?)?;
    let string = string_place(&namespace)?;
    let postal = postal_place(&namespace)?;
    broker.entities().create(&string).send().await?;
    broker.entities().create(&postal).send().await?;
    let string_body = retrieved_ld(&broker, &string.id).await?;
    expect_that!(string_body.pointer("/address/valueType"), some(eq(&json!("String"))));
    let body = retrieved_ld(&broker, &postal.id).await?;
    expect_with_any_context(
        body,
        &json!({
            "id": postal.id.to_string(),
            "type": namespace.suffixed("Place"),
            "address": {
                "type": "Property",
                "value": {"street": "Straße des 17. Juni", "city": "Berlin", "country": "Germany"},
                "valueType": "PostalAddress",
            },
        }),
    );
    served.stop();
    Ok(())
}
