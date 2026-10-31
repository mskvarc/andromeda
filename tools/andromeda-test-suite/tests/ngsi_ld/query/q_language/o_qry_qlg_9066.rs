//! A value list of unquoted uris selects by any of them.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    raw_call::{call, target},
    unchecked_page::entities,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, Response, StatusCode};
use ngsi_ld::model::{Entity, EntityId, EntityTypeName, Relationship};

/// Query Entities by GET for the type `shop_type` with the query text `q`, as written.
async fn get_shops(broker: &BrokerClient, shop_type: &EntityTypeName, q: &str) -> Result<Response<Bytes>> {
    // The query language of the model quotes every URI of a list, so the bare list is sent as written.
    let shop_type_text = shop_type.to_string();
    Ok(call(
        broker,
        Method::GET,
        &target("entities", &[("type", &shop_type_text), ("q", q)]),
        &[("Accept", "application/json")],
        None,
    )
    .await?)
}

/// Checks that the answer is a 200 holding exactly the entities `ids`.
fn expect_exactly(response: &Response<Bytes>, ids: &[&EntityId]) -> Result<()> {
    verify_that!(response.status(), eq(StatusCode::OK))?;
    let found = entities(response)?;
    expect_that!(found, len(eq(ids.len())));
    for id in ids {
        expect_that!(found, contains(has_id(*id)));
    }
    Ok(())
}

/// Pins: a value list of unquoted URIs after == selects the entities whose Relationship object is any of them and after
/// != those whose object is none of them, as the quoted list does, by GET and by a POST query
/// Clause: 4.9, 5.7.2.4, 6.4.3.2, 6.23.3.1
#[gtest]
#[tokio::test]
async fn o_qry_qlg_9066_a_value_list_of_unquoted_uris_selects_by_any_of_them() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let pisa_id = namespace.urn("Shop", "pisa")?;
    let lucca_id = namespace.urn("Shop", "lucca")?;
    let siena_id = namespace.urn("Shop", "siena")?;
    let shop_type = namespace.entity_type("Shop")?;
    let entities = ["pisa", "lucca", "siena"]
        .into_iter()
        .map(|city| {
            Ok(Entity::builder()
                .id(namespace.urn("Shop", city)?)
                .entity_type(shop_type.clone())
                .relationship("locatedIn", Relationship::builder(namespace.urn("City", city)?))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let pisa_lucca = format!("urn:ngsi-ld:City:{namespace}-pisa,urn:ngsi-ld:City:{namespace}-lucca");
    let bare = get_shops(&broker, &shop_type, &format!("locatedIn=={pisa_lucca}")).await?;
    expect_exactly(&bare, &[&pisa_id, &lucca_id])?;
    let quoted = get_shops(
        &broker,
        &shop_type,
        &format!(r#"locatedIn=="urn:ngsi-ld:City:{namespace}-pisa","urn:ngsi-ld:City:{namespace}-lucca""#),
    )
    .await?;
    expect_exactly(&quoted, &[&pisa_id, &lucca_id])?;
    let unequal = get_shops(&broker, &shop_type, &format!("locatedIn!={pisa_lucca}")).await?;
    expect_exactly(&unequal, &[&siena_id])?;
    let body = format!(
        r#"{{"type": "Query", "entities": [{{"type": "{shop_type}"}}], "q": "locatedIn==urn:ngsi-ld:City:{namespace}-siena,\"urn:ngsi-ld:City:{namespace}-lucca\""}}"#
    );
    let posted = call(&broker, Method::POST, "entityOperations/query", &[("Accept", "application/json")], Some(&body)).await?;
    expect_exactly(&posted, &[&siena_id, &lucca_id])?;
    Ok(())
}
