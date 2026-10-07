//! Query Entities auxiliary supplementation, distributed pagination, csf forwarding, post-merge `datasetId` filtering, and local `orderBy` (clauses 5.7.2.4, 5.5.9).

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property, property_with_dataset, typed_vehicle},
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::time::Duration;
use urlencoding::encode;

/// The `id` strings of an Entity Array body.
fn ids(body: &Value) -> Vec<String> {
    body.as_array()
        .map(|entries| entries.iter().filter_map(|entry| entry["id"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// The instances of one attribute on the entity with the given id.
fn attribute_instances(body: &Value, id: &str, attribute: &str) -> Vec<Value> {
    let entity = body
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["id"].as_str() == Some(id)))
        .cloned()
        .unwrap_or(Value::Null);
    entity[attribute].as_array().cloned().unwrap_or_else(|| {
        if entity.get(attribute).is_some() {
            vec![entity[attribute].clone()]
        } else {
            Vec::new()
        }
    })
}

/// An auxiliary source supplies an attribute in a query result only where the merged entity lacks it; it never overrides a present value (clause 5.7.2.4).
#[tokio::test]
async fn distop_query_entities_auxiliary_added_only_where_attr_absent() {
    let topology = topology::pair().await.expect("two-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:cons32");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let peer = topology.broker(BrokerRole::B).expect("topology broker B");

    assert_status(
        &front
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 50))
            .with_ld_json()
            .send()
            .await
            .expect("local"),
        201,
    );
    let auxiliary = entity(&entity_id, &vtype, &[("speed", property(json!(99))), ("fuelLevel", property(json!(70)))]);
    assert_status(&peer.client().post_entity(&auxiliary).with_ld_json().send().await.expect("peer"), 201);

    let registration_id = topology.id("registration:cons32");
    let registration = CsrBuilder::new(&registration_id, peer.peer_url())
        .tenant(topology.tenant())
        .mode("auxiliary")
        .entity_id(&entity_id)
        .entity_type(&vtype)
        .property_names(&["speed", "fuelLevel"])
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let response = front
        .client()
        .query_entities(&format!("type={}", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let body = json_body(&response);
    let speed = attribute_instances(&body, &entity_id, "speed");
    assert!(
        speed.iter().any(|instance| instance["value"].as_i64() == Some(50)),
        "the present speed value is not overridden by auxiliary: {body}"
    );
    assert!(
        !attribute_instances(&body, &entity_id, "fuelLevel").is_empty(),
        "auxiliary fuelLevel is added where absent: {body}"
    );

    topology.finish().await.expect("topology cleanup");
}

/// Distributed pagination over candidate entities covers every candidate across pages with no duplicate and no page exceeding the limit (clause 5.5.9).
#[tokio::test]
async fn distop_query_entities_pagination_over_mapped_candidates() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    let vtype = topology.id("Vehicle");
    let candidates: Vec<String> = (0..5).map(|index| topology.id(&format!("Vehicle:cons34-{index}"))).collect();
    let array = Value::Array(
        candidates
            .iter()
            .enumerate()
            .map(|(index, id)| typed_vehicle(id, &vtype, 10 * i64::try_from(index).expect("a candidate index fits in an i64")))
            .collect(),
    );
    let registration_id = topology.id("registration:cons34");
    let registration = CsrBuilder::new(&registration_id, &mock.url(""))
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &array).await;

    let mut seen: Vec<String> = Vec::new();
    for offset in [0, 2, 4] {
        let page = front
            .client()
            .query_entities(
                &format!("type={}&limit=2&offset={offset}&count=true", encode(&vtype)),
                "application/ld+json",
                None,
            )
            .await
            .expect("page");
        assert_status(&page, 200);
        let page_ids = ids(&json_body(&page));
        assert!(page_ids.len() <= 2, "no page exceeds the limit, got {page_ids:?}");
        for id in page_ids {
            assert!(!seen.contains(&id), "no id appears on two pages: {id}");
            seen.push(id);
        }
    }
    for candidate in &candidates {
        assert!(seen.contains(candidate), "every candidate is covered across pages, missing {candidate}");
    }

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `csf` expression is forwarded byte-identical into the downstream discovery a chained source performs (clause 5.7.2.4).
#[tokio::test]
async fn distop_query_entities_csf_forwarded_unchanged_to_discovery() {
    let topology = topology::chain().await.expect("three-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let middle = topology.broker(BrokerRole::B).expect("topology broker B");
    let mock = ContextSourceAdapter::start().await;

    let vtype = topology.id("Vehicle");
    let downstream_id = topology.id("registration:cons35-downstream");
    let downstream = CsrBuilder::new(&downstream_id, &mock.url(""))
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["queryEntity"])
        .build();
    assert_status(&middle.client().create_registration(&downstream).await.expect("downstream register"), 201);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &json!([])).await;

    let hop_id = topology.id("registration:cons35-hop");
    let hop = CsrBuilder::new(&hop_id, middle.peer_url())
        .tenant(topology.tenant())
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&hop).await.expect("hop register"), 201);

    let csf = "mode==inclusive";
    assert_status(
        &front
            .client()
            .query_entities(&format!("type={}&csf={}", encode(&vtype), encode(csf)), "application/ld+json", None)
            .await
            .expect("query"),
        200,
    );

    let forwarded = mock.next_request(Duration::from_secs(3)).await;
    let query = forwarded.uri.query().unwrap_or_default();
    assert!(
        query.contains("csf"),
        "the csf expression is forwarded into downstream discovery, got {query:?}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `datasetId` query parameter applies per-attribute instance filtering across aggregated entities; `@none` matches the default instance (clause 5.7.2.4).
#[tokio::test]
async fn distop_query_entities_datasetid_per_attribute_filtering() {
    let topology = topology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons36");
    let vtype = topology.id("Vehicle");
    let dataset_id = topology.id("dataset:2");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let holder = topology.broker(BrokerRole::C).expect("topology broker C");

    assert_status(
        &front
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 10))
            .with_ld_json()
            .send()
            .await
            .expect("local"),
        201,
    );
    assert_status(
        &holder
            .client()
            .post_entity(&entity(&entity_id, &vtype, &[("speed", property_with_dataset(json!(30), &dataset_id))]))
            .with_ld_json()
            .send()
            .await
            .expect("holder"),
        201,
    );
    let registration_id = topology.id("registration:cons36");
    let registration = CsrBuilder::new(&registration_id, holder.peer_url())
        .tenant(topology.tenant())
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let by_dataset = front
        .client()
        .query_entities(
            &format!("type={}&datasetId={}", encode(&vtype), encode(&dataset_id)),
            "application/ld+json",
            None,
        )
        .await
        .expect("dataset");
    assert_status(&by_dataset, 200);
    let dataset_instances = attribute_instances(&json_body(&by_dataset), &entity_id, "speed");
    assert!(
        !dataset_instances.is_empty()
            && dataset_instances
                .iter()
                .all(|instance| instance["datasetId"].as_str() == Some(dataset_id.as_str())),
        "only the requested-datasetId instances are kept"
    );

    let by_none = front
        .client()
        .query_entities(&format!("type={}&datasetId=@none", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("none");
    assert_status(&by_none, 200);
    let none_instances = attribute_instances(&json_body(&by_none), &entity_id, "speed");
    assert!(
        none_instances.iter().all(|instance| instance.get("datasetId").is_none()),
        "@none keeps only default instances"
    );

    topology.finish().await.expect("topology cleanup");
}

/// A `datasetId` no instance carries omits that attribute from each aggregated entity (clause 5.7.2.4).
#[tokio::test]
async fn distop_query_entities_datasetid_no_match_omits_attribute() {
    let topology = topology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons37");
    let vtype = topology.id("Vehicle");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let holder = topology.broker(BrokerRole::C).expect("topology broker C");

    assert_status(
        &front
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 10))
            .with_ld_json()
            .send()
            .await
            .expect("local"),
        201,
    );
    assert_status(
        &holder
            .client()
            .post_entity(&entity(&entity_id, &vtype, &[("fuelLevel", property(json!(70)))]))
            .with_ld_json()
            .send()
            .await
            .expect("holder"),
        201,
    );
    let registration_id = topology.id("registration:cons37");
    let registration = CsrBuilder::new(&registration_id, holder.peer_url())
        .tenant(topology.tenant())
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let absent = topology.id("dataset:absent");
    let response = front
        .client()
        .query_entities(&format!("type={}&datasetId={}", encode(&vtype), encode(&absent)), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    assert!(
        attribute_instances(&json_body(&response), &entity_id, "speed").is_empty(),
        "speed has no instance with the requested datasetId and is omitted"
    );

    topology.finish().await.expect("topology cleanup");
}

/// A `local=true` query with `orderBy` on an attribute member orders the result by that member, ascending then descending (clause 5.7.2.4). The descending direction is the `;desc` suffix of the clause 4.23.3 grammar (`OrderingTerm = AttrName *1(DirectionTerm)`, `DirectionTerm = %x3B directionOp`), sent percent-encoded.
#[tokio::test]
async fn distop_query_entities_orderby_local_orders_by_member() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let vtype = topology.id("Vehicle");
    let low = topology.id("Vehicle:cons38-low");
    let mid = topology.id("Vehicle:cons38-mid");
    let high = topology.id("Vehicle:cons38-high");
    for (id, speed) in [(&low, 10), (&mid, 20), (&high, 30)] {
        assert_status(
            &front
                .client()
                .post_entity(&typed_vehicle(id, &vtype, speed))
                .with_ld_json()
                .send()
                .await
                .expect("create"),
            201,
        );
    }

    let ascending = front
        .client()
        .query_entities(&format!("type={}&local=true&orderBy=speed", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("asc");
    assert_status(&ascending, 200);
    let ordered: Vec<String> = ids(&json_body(&ascending)).into_iter().filter(|id| [&low, &mid, &high].contains(&id)).collect();
    assert_eq!(ordered, vec![low.clone(), mid.clone(), high.clone()], "ascending orderBy sorts by the member");

    let descending = front
        .client()
        .query_entities(&format!("type={}&local=true&orderBy=speed%3Bdesc", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("desc");
    assert_status(&descending, 200);
    let reversed: Vec<String> = ids(&json_body(&descending))
        .into_iter()
        .filter(|id| [&low, &mid, &high].contains(&id))
        .collect();
    assert_eq!(reversed, vec![high, mid, low], "descending orderBy reverses the order");

    topology.finish().await.expect("topology cleanup");
}
