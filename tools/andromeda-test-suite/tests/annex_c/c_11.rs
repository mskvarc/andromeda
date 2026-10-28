//! Annex C.11: an attribute signed with an `ngsildproof`, a W3C Data Integrity proof sealed to its entity
//! (clause 4.5.2.2).

use crate::{
    printed_document::expect_with_any_context,
    served_context::{ServedContexts, linked, retrieved_ld},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use indexmap::IndexMap;
use ngsi_ld::model::{
    Entity,
    EntityFragment,
    GeoProperty,
    Input,
    Proof,
    Property,
    Value,
    geometry::{Position, Shape},
};
use serde_json::{Value as Json, json};
use url::Url;

/// Serves the example's `@context`, the Store type carrying the namespace.
async fn store_context(served: &ServedContexts, namespace: &Namespace) -> Result<Url> {
    let store = namespace.suffixed("Store");
    let document = json!({"@context": {
        store.as_str(): format!("https://uri.etsi.org/ngsi-ld/primer/{store}"),
        "address": "https://schema.org/address",
        "streetAddress": "https://schema.org/streetAddress",
        "addressRegion": "https://schema.org/addressRegion",
        "addressLocality": "https://schema.org/addressLocality",
        "postalCode": "https://schema.org/postalCode",
    }});
    served.serve("store-context.jsonld", &document).await
}

/// The Store `002` of the example, its address signed and its location the point (57.5522, -20.3484).
fn signed_store(namespace: &Namespace) -> Result<Entity<Input>> {
    let id = namespace.urn("Store", "002")?;
    let signature = Value::Object(IndexMap::from([
        ("type".to_owned(), Value::from("DataIntegrityProof")),
        ("created".to_owned(), Value::from("2025-01-27T21:02:24Z")),
        (
            "verificationMethod".to_owned(),
            Value::from("https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT"),
        ),
        ("cryptosuite".to_owned(), Value::from("eddsa-rdfc-2022")),
        ("proofPurpose".to_owned(), Value::from("assertionMethod")),
        (
            "proofValue".to_owned(),
            Value::from("z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"),
        ),
    ]));
    let proof = Proof::builder(signature)
        .entity_id_sealed(id.as_str())
        .entity_type_sealed(namespace.suffixed("Store").as_str())
        .build()?;
    let address = Value::Object(IndexMap::from([
        ("streetAddress".to_owned(), Value::Array(vec![Value::from("Tiger Street 4"), Value::from("al")])),
        ("addressRegion".to_owned(), Value::from("Metropolis")),
        ("addressLocality".to_owned(), Value::from("Cat City")),
        ("postalCode".to_owned(), Value::from("42420")),
    ]));
    Ok(Entity::builder()
        .id(id)
        .entity_type(namespace.entity_type("Store")?)
        .property("address", Property::builder(address).proof(proof))
        .geo_property("location", Shape::Point(Position::try_new(57.5522, -20.3484)?))
        .build()?)
}

/// The `ngsildproof` of the example's address, as the broker answers it.
fn printed_proof(store: &Entity<Input>, namespace: &Namespace) -> Json {
    json!({
        "type": "Property",
        "entityIdSealed": store.id.to_string(),
        "entityTypeSealed": namespace.suffixed("Store"),
        "value": {
            "type": "DataIntegrityProof",
            "created": "2025-01-27T21:02:24Z",
            "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT",
            "cryptosuite": "eddsa-rdfc-2022",
            "proofPurpose": "assertionMethod",
            "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS",
        },
    })
}

/// The example's signed address, as the broker answers it.
fn printed_address(store: &Entity<Input>, namespace: &Namespace) -> Json {
    json!({
        "type": "Property",
        "value": {
            "streetAddress": ["Tiger Street 4", "al"],
            "addressRegion": "Metropolis",
            "addressLocality": "Cat City",
            "postalCode": "42420",
        },
        "ngsildproof": printed_proof(store, namespace),
    })
}

/// Pins: The Store whose address carries an ngsildproof is retrieved exactly as C.11 prints it
/// Clause: C.11, 4.5.2.2, 5.7.1
#[gtest]
#[tokio::test]
async fn c_11_0001_a_signed_attribute_round_trips_with_its_proof() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &store_context(&served, &namespace).await?)?;
    let store = signed_store(&namespace)?;
    broker.entities().create(&store).send().await?;
    let body = retrieved_ld(&broker, &store.id).await?;
    expect_with_any_context(
        body,
        &json!({
            "id": store.id.to_string(),
            "type": namespace.suffixed("Store"),
            "address": printed_address(&store, &namespace),
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [57.5522, -20.3484]}},
        }),
    );
    served.stop();
    Ok(())
}

/// Pins: The retrieved ngsildproof is a Property sealing the Store id and type whose value is the eddsa-rdfc-2022
/// Data Integrity proof C.11 prints, unaltered
/// Clause: C.11, 4.5.2.2, 5.7.1
#[gtest]
#[tokio::test]
async fn c_11_0002_the_proof_carries_the_printed_data_integrity_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &store_context(&served, &namespace).await?)?;
    let store = signed_store(&namespace)?;
    broker.entities().create(&store).send().await?;
    let body = retrieved_ld(&broker, &store.id).await?;
    expect_that!(body.pointer("/address/ngsildproof"), some(eq(&printed_proof(&store, &namespace))));
    served.stop();
    Ok(())
}

/// Pins: Only address carries a proof, and an update of location leaves address and its proof as they were
/// Clause: C.11, 4.5.2.2, 5.6.2, 5.7.1
#[gtest]
#[tokio::test]
async fn c_11_0003_an_update_of_another_attribute_leaves_the_signed_one_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &store_context(&served, &namespace).await?)?;
    let store = signed_store(&namespace)?;
    broker.entities().create(&store).send().await?;
    let body = retrieved_ld(&broker, &store.id).await?;
    expect_that!(body.pointer("/location/type"), some(eq(&json!("GeoProperty"))));
    expect_that!(body.pointer("/location/ngsildproof"), none());
    let fragment = EntityFragment::builder()
        .geo_property("location", GeoProperty::builder(Shape::Point(Position::try_new(1.0, 2.0)?)))
        .build()?;
    let updated = broker.with_responses().entities().update_attributes(&store.id, &fragment).send().await;
    expect_that!(&updated, answered(StatusCode::NO_CONTENT));
    let body = retrieved_ld(&broker, &store.id).await?;
    expect_that!(body.pointer("/location/value/coordinates/0").and_then(Json::as_f64), some(eq(1.0)));
    expect_that!(body.pointer("/address"), some(eq(&printed_address(&store, &namespace))));
    served.stop();
    Ok(())
}
