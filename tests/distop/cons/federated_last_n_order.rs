//! `lastN` over a merged temporal history (clause 5.7.3.4): the clause fixes which `n` instances the answer keeps -- the ones at the last `n` instants of the merged history -- and not their order, so the answer lists them in the one order every local read lists instances in, oldest first.

use crate::{
    entity::entity,
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// A single temporal `Property` instance at the given `observedAt`.
fn observed(value: i64, observed_at: &str) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": observed_at })
}

/// The whole-history temporal query window: everything before 2030.
const BEFORE_2030: &str = "timerel=before&timeAt=2030-01-01T00:00:00Z";

/// The `observedAt` values of one attribute's instances, in answer order.
fn observed_ats(body: &Value, attribute: &str) -> Vec<String> {
    body[attribute]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|instance| instance["observedAt"].as_str().map(str::to_owned))
        .collect()
}

/// Two holders contribute older and newer instances; the front broker answers `lastN=3` over the merged history and lists the kept instances oldest first, as a read that took no source would.
#[tokio::test]
async fn distop_federated_last_n_keeps_the_newest_instances_in_ascending_order() {
    let topology = topology::pair().await.expect("two-broker topology");
    let entity_id = topology.id("Vehicle:federated-last-n-order");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let holder = topology.broker(BrokerRole::B).expect("topology broker B");

    assert_status(
        &front
            .client()
            .create_temporal_entity(&entity(
                &entity_id,
                "Vehicle",
                &[("speed", json!([observed(10, "2020-08-01T12:00:00Z"), observed(20, "2020-08-01T12:02:00Z")]))],
            ))
            .await
            .expect("local"),
        201,
    );
    assert_status(
        &holder
            .client()
            .create_temporal_entity(&entity(
                &entity_id,
                "Vehicle",
                &[("speed", json!([observed(30, "2020-08-01T12:01:00Z"), observed(40, "2020-08-01T12:03:00Z")]))],
            ))
            .await
            .expect("holder"),
        201,
    );
    let registration = CsrBuilder::new(&topology.id("registration:federated-last-n-order"), holder.peer_url())
        .tenant(topology.tenant())
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveTemporal"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let retrieved = front
        .client()
        .get_temporal_entity(&entity_id, &format!("{BEFORE_2030}&attrs=speed&lastN=3"), None)
        .await
        .expect("retrieve");
    assert_status(&retrieved, 200);
    assert_eq!(
        observed_ats(&json_body(&retrieved), "speed"),
        vec![
            "2020-08-01T12:01:00Z".to_owned(),
            "2020-08-01T12:02:00Z".to_owned(),
            "2020-08-01T12:03:00Z".to_owned()
        ],
        "the last three instants of the merged history, oldest first"
    );

    topology.finish().await.expect("topology cleanup");
}
