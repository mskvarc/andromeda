//! Query-CSR entity and attribute selection (clause 5.10.2 over the 5.12
//! matching rules): term expansion, the entity spec, attribute-name matching,
//! the unconstrained sub-cases, conjunction of conditions, and `RegistrationInfo`
//! trimming. All `T-LOCAL`.

use crate::{
    disc::{
        PEER_ENDPOINT,
        csr_discovery::{contains, find, query, register},
    },
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use pretty_assertions::assert_eq;
use serde_json::json;
use urlencoding::encode;

/// Attribute names that appear only inside `q` are used as the attribute list:
/// `q=speed>10` matches the registration that covers `speed` and excludes the
/// one that covers a different property (clause 5.10.2). Confirms REQ-DISC-19.
#[tokio::test]
async fn query_csr_attributes_referenced_only_in_q_used_as_attribute_list() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("QType");
    let speed_csr = topology.id("registration:q-speed");
    let brand_csr = topology.id("registration:q-brand");
    register(
        &topology,
        &CsrBuilder::new(&speed_csr, PEER_ENDPOINT)
            .entity_type(&vtype)
            .property_names(&["speed"])
            .build(),
    )
    .await;
    register(
        &topology,
        &CsrBuilder::new(&brand_csr, PEER_ENDPOINT)
            .entity_type(&vtype)
            .property_names(&["brandName"])
            .build(),
    )
    .await;

    let body = query(&topology, &format!("type={}&q=speed%3E10", encode(&vtype))).await;
    assert!(contains(&body, &speed_csr), "the speed registration matches a q over speed");
    assert!(!contains(&body, &brand_csr), "the brandName registration is excluded");
    topology.finish().await.expect("topology cleanup");
}

/// Short-hand type terms are expanded before matching: a `Vehicle` query with
/// the compound context matches the registration, while the same query without
/// a context expands differently and does not (clause 5.5.7). Confirms
/// REQ-DISC-22.
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn query_csr_expands_shorthand_type_and_attr_terms_before_matching() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:expand-match");
    // The literal `Vehicle` shorthand is the point of this test (it must expand
    // under the request context), so a prefix `idPattern` scopes the registration
    // to this test's own ids instead, keeping it from matching another concurrent
    // test's `Vehicle` writes on the shared broker; the shared-type serial key
    // keeps the bare `type=Vehicle` discovery query from seeing another such test.
    register(
        &topology,
        &CsrBuilder::new(&id, PEER_ENDPOINT)
            .entity_type("Vehicle")
            .id_pattern(&format!("{}.*", topology.id_prefix()))
            .build(),
    )
    .await;

    let with_context = query(&topology, "type=Vehicle").await;
    assert!(contains(&with_context, &id), "the compound context expands Vehicle to the registered IRI");

    let without_context = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .without_default_context()
        .query_registrations("type=Vehicle", None)
        .await
        .expect("query without context");
    assert_status(&without_context, 200);
    assert!(
        !contains(&json_body(&without_context), &id),
        "without a context Vehicle expands differently and does not match"
    );
    topology.finish().await.expect("topology cleanup");
}

/// A Query-CSR returns only registrations meeting every condition: the
/// conjunction of type and attribute list excludes a registration that fails
/// one part (clause 5.10.2). Confirms REQ-DISC-23.
#[tokio::test]
async fn query_csr_returns_only_csrs_meeting_all_conditions() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("AllType");
    let both = topology.id("registration:all-both");
    let one = topology.id("registration:all-one");
    register(
        &topology,
        &CsrBuilder::new(&both, PEER_ENDPOINT).entity_type(&vtype).property_names(&["speed"]).build(),
    )
    .await;
    register(
        &topology,
        &CsrBuilder::new(&one, PEER_ENDPOINT).entity_type(&vtype).property_names(&["brandName"]).build(),
    )
    .await;

    let body = query(&topology, &format!("type={}&attrs=speed", encode(&vtype))).await;
    assert!(contains(&body, &both), "the registration satisfying type and attribute matches");
    assert!(!contains(&body, &one), "the registration failing the attribute condition is excluded");
    topology.finish().await.expect("topology cleanup");
}

/// The entity spec matches `EntityInfo` on type and idPattern: an id covered by
/// the registered pattern matches, an id outside it does not (clause 5.12).
/// Confirms REQ-DISC-24.
#[tokio::test]
async fn query_csr_entity_spec_matches_entityinfo() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("PatType");
    let id = topology.id("registration:pattern");
    let covered = topology.id("Match:1");
    let outside = topology.id("Other:1");
    register(
        &topology,
        &CsrBuilder::new(&id, PEER_ENDPOINT)
            .entity_type(&vtype)
            .id_pattern(&format!("{}.*", topology.id("Match")))
            .build(),
    )
    .await;

    let matching = query(&topology, &format!("type={}&id={}", encode(&vtype), encode(&covered))).await;
    assert!(contains(&matching, &id), "an id covered by the registered idPattern matches");

    let non_matching = query(&topology, &format!("type={}&id={}", encode(&vtype), encode(&outside))).await;
    assert!(!contains(&non_matching, &id), "an id outside the registered idPattern does not match");
    topology.finish().await.expect("topology cleanup");
}

/// A registration with no `EntityInfo` is unconstrained on the entity part and
/// matches any entity condition (clause 5.12). Confirms REQ-DISC-25.
#[tokio::test]
async fn query_csr_no_entityinfo_entity_spec_considered_matching() {
    let topology = topology::single().await.expect("single-broker topology");
    let attr = topology.id("attr:entityless");
    let id = topology.id("registration:entityless");
    register(
        &topology,
        &CsrBuilder::new(&id, PEER_ENDPOINT).without_entity().property_names(&[attr.as_str()]).build(),
    )
    .await;

    let body = query(&topology, &format!("attrs={}", encode(&attr))).await;
    assert!(
        contains(&body, &id),
        "an entity-less registration matches on its attribute regardless of entity constraints"
    );
    topology.finish().await.expect("topology cleanup");
}

/// An attribute name matches whether the registration covers it as a Property
/// or a Relationship (clause 5.12). Confirms REQ-DISC-26.
#[tokio::test]
async fn query_csr_attribute_name_matches_property_or_relationship() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("RelType");
    let rel = topology.id("rel:parkedIn");
    let id = topology.id("registration:relationship");
    register(
        &topology,
        &CsrBuilder::new(&id, PEER_ENDPOINT)
            .entity_type(&vtype)
            .relationship_names(&[rel.as_str()])
            .build(),
    )
    .await;

    let body = query(&topology, &format!("type={}&attrs={}", encode(&vtype), encode(&rel))).await;
    assert!(contains(&body, &id), "an attribute list matches a registered relationship name");
    topology.finish().await.expect("topology cleanup");
}

/// A registration that registers no attribute names is unconstrained on the
/// attribute part and matches any attribute list (clause 5.12). Confirms
/// REQ-DISC-27.
#[tokio::test]
async fn query_csr_no_registered_attrs_attribute_names_considered_matching() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("AnyAttrType");
    let id = topology.id("registration:any-attr");
    register(&topology, &CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).build()).await;

    let body = query(&topology, &format!("type={}&attrs=speed", encode(&vtype))).await;
    assert!(contains(&body, &id), "an entity-only registration matches any requested attribute list");
    topology.finish().await.expect("topology cleanup");
}

/// A returned registration is trimmed to only the `RegistrationInfo` entries that
/// match the query (clause 5.10.2, SHOULD). Confirms REQ-DISC-41.
#[tokio::test]
async fn query_csr_returns_registrationinfo_trimmed_to_matching() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("TrimType");
    let other_type = topology.id("TrimOther");
    let id = topology.id("registration:trim");
    let mut body = CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).build();
    let extra = json!({ "entities": [ { "type": other_type } ] });
    body["information"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("information is an array"))
        .push(extra);
    register(&topology, &body).await;

    let result = query(&topology, &format!("type={}", encode(&vtype))).await;
    let registration = find(&result, &id).unwrap_or_else(|| panic!("the registration is returned"));
    let information = registration["information"].as_array().unwrap_or_else(|| panic!("information is an array"));
    assert_eq!(
        information.len(),
        1,
        "only the matching RegistrationInfo entry is returned, got {information:?}"
    );
    topology.finish().await.expect("topology cleanup");
}
