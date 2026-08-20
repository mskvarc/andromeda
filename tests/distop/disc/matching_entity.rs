//! Matching algorithm, entity part (clause 5.12): the relevant-spec inputs, the
//! any-RegistrationInfo rule, the both-parts rule, and the id sub-cases,
//! exercised through the discovery-query surface. All `T-LOCAL`.

use crate::{
    disc::{
        PEER_ENDPOINT,
        csr_discovery::{contains, query, register},
    },
    registration::CsrBuilder,
    topology::DistributedTopology,
};
use serde_json::json;
use urlencoding::encode;

/// The relevant-spec inputs (type, id, idPattern, attribute list) each narrow
/// the matched registrations independently (clause 5.12). Confirms REQ-DISC-94.
#[tokio::test]
async fn matching_query_relevant_spec_uses_type_ids_idpattern_attrs() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("SpecType");
    let covered = topology.id("Vehicle:covered");
    let other = topology.id("Vehicle:other");
    let id = topology.id("registration:spec");
    register(
        &topology,
        &CsrBuilder::new(&id, PEER_ENDPOINT)
            .entity_type(&vtype)
            .entity_id(&covered)
            .property_names(&["speed"])
            .build(),
    )
    .await;

    let by_id = query(&topology, &format!("type={}&id={}", encode(&vtype), encode(&covered))).await;
    assert!(contains(&by_id, &id), "the matching entity id keeps the registration");

    let by_other_id = query(&topology, &format!("type={}&id={}", encode(&vtype), encode(&other))).await;
    assert!(!contains(&by_other_id, &id), "a different entity id narrows the registration away");

    let by_attr = query(&topology, &format!("type={}&id={}&attrs=brandName", encode(&vtype), encode(&covered))).await;
    assert!(!contains(&by_attr, &id), "an unregistered attribute list narrows the registration away");
    topology.finish().await.expect("topology cleanup");
}

/// A query spec matches a registration if any of its RegistrationInfo entries
/// matches (clause 5.12). Confirms REQ-DISC-99.
#[tokio::test]
async fn matching_spec_matches_csr_if_any_registrationinfo_matches() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let first_type = topology.id("AnyInfoFirst");
    let second_type = topology.id("AnyInfoSecond");
    let id = topology.id("registration:any-info");
    let mut body = CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&first_type).build();
    body["information"]
        .as_array_mut()
        .unwrap_or_else(|| unreachable!("information is an array"))
        .push(json!({ "entities": [ { "type": second_type } ] }));
    register(&topology, &body).await;

    let matched = query(&topology, &format!("type={}", encode(&second_type))).await;
    assert!(contains(&matched, &id), "matching the second RegistrationInfo matches the registration");

    let unrelated = query(&topology, &format!("type={}", encode(&topology.id("AnyInfoUnrelated")))).await;
    assert!(!contains(&unrelated, &id), "a type matching no RegistrationInfo does not match");
    topology.finish().await.expect("topology cleanup");
}

/// A RegistrationInfo matches only when both its entity part and its attribute
/// part match (clause 5.12). Confirms REQ-DISC-100.
#[tokio::test]
async fn matching_registrationinfo_requires_both_entity_and_attribute_parts() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("BothPartsType");
    let other_type = topology.id("BothPartsOther");
    let id = topology.id("registration:both-parts");
    register(
        &topology,
        &CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).property_names(&["speed"]).build(),
    )
    .await;

    let both = query(&topology, &format!("type={}&attrs=speed", encode(&vtype))).await;
    assert!(contains(&both, &id), "matching both the entity and attribute parts matches");

    let attribute_fails = query(&topology, &format!("type={}&attrs=brandName", encode(&vtype))).await;
    assert!(!contains(&attribute_fails, &id), "failing the attribute part excludes the registration");

    let entity_fails = query(&topology, &format!("type={}&attrs=speed", encode(&other_type))).await;
    assert!(!contains(&entity_fails, &id), "failing the entity part excludes the registration");
    topology.finish().await.expect("topology cleanup");
}

/// The entity part requires both a type selector and the id condition to hold
/// (clause 5.12). Confirms REQ-DISC-101.
#[tokio::test]
async fn matching_entity_requires_type_selector_and_id_condition() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("EntityPartType");
    let other_type = topology.id("EntityPartOther");
    let covered = topology.id("Vehicle:entity-part");
    let outside = topology.id("Vehicle:entity-part-other");
    let id = topology.id("registration:entity-part");
    register(&topology, &CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).entity_id(&covered).build()).await;

    let both = query(&topology, &format!("type={}&id={}", encode(&vtype), encode(&covered))).await;
    assert!(contains(&both, &id), "matching type and id keeps the registration");

    let wrong_type = query(&topology, &format!("type={}&id={}", encode(&other_type), encode(&covered))).await;
    assert!(!contains(&wrong_type, &id), "a failing type selector excludes the registration");

    let wrong_id = query(&topology, &format!("type={}&id={}", encode(&vtype), encode(&outside))).await;
    assert!(!contains(&wrong_id, &id), "a failing id condition excludes the registration");
    topology.finish().await.expect("topology cleanup");
}

/// Id sub-case: EntityInfo with neither id nor idPattern matches any queried id
/// (clause 5.12). Confirms REQ-DISC-102.
#[tokio::test]
async fn matching_id_subcase_entityinfo_without_id_or_idpattern_matches() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("NoIdType");
    let id = topology.id("registration:no-id");
    register(&topology, &CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).build()).await;

    let body = query(&topology, &format!("type={}&id={}", encode(&vtype), encode(&topology.id("Vehicle:anything")))).await;
    assert!(contains(&body, &id), "an EntityInfo without id or idPattern matches any queried id");
    topology.finish().await.expect("topology cleanup");
}

/// Id sub-case: a specified id equal to EntityInfo.id matches, a different id
/// does not (clause 5.12). Confirms REQ-DISC-103.
#[tokio::test]
async fn matching_id_subcase_specified_id_equals_entityinfo_id() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("IdEqType");
    let covered = topology.id("Vehicle:id-eq");
    let other = topology.id("Vehicle:id-eq-other");
    let id = topology.id("registration:id-eq");
    register(&topology, &CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).entity_id(&covered).build()).await;

    let equal = query(&topology, &format!("type={}&id={}", encode(&vtype), encode(&covered))).await;
    assert!(contains(&equal, &id), "a queried id equal to EntityInfo.id matches");

    let different = query(&topology, &format!("type={}&id={}", encode(&vtype), encode(&other))).await;
    assert!(!contains(&different, &id), "a queried id different from EntityInfo.id does not match");
    topology.finish().await.expect("topology cleanup");
}

/// Id sub-case: a specified id that matches EntityInfo.idPattern matches, one
/// outside it does not (clause 5.12). Confirms REQ-DISC-104.
#[tokio::test]
async fn matching_id_subcase_specified_id_matches_entityinfo_idpattern() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("IdPatType");
    let covered = topology.id("Fleet:1");
    let outside = topology.id("Other:1");
    let id = topology.id("registration:id-pattern");
    register(
        &topology,
        &CsrBuilder::new(&id, PEER_ENDPOINT)
            .entity_type(&vtype)
            .id_pattern(&format!("{}.*", topology.id("Fleet")))
            .build(),
    )
    .await;

    let matching = query(&topology, &format!("type={}&id={}", encode(&vtype), encode(&covered))).await;
    assert!(contains(&matching, &id), "a queried id covered by EntityInfo.idPattern matches");

    let non_matching = query(&topology, &format!("type={}&id={}", encode(&vtype), encode(&outside))).await;
    assert!(!contains(&non_matching, &id), "a queried id outside EntityInfo.idPattern does not match");
    topology.finish().await.expect("topology cleanup");
}

/// Id sub-case: a specified idPattern that matches EntityInfo.id matches, a
/// disjoint pattern does not (clause 5.12). Confirms REQ-DISC-105.
#[tokio::test]
async fn matching_id_subcase_specified_idpattern_matches_entityinfo_id() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("QueryPatType");
    let covered = topology.id("Fleet:covered");
    let id = topology.id("registration:query-pattern");
    register(&topology, &CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).entity_id(&covered).build()).await;

    let matching = query(
        &topology,
        &format!("type={}&idPattern={}", encode(&vtype), encode(&format!("{}.*", topology.id("Fleet")))),
    )
    .await;
    assert!(contains(&matching, &id), "a queried idPattern covering EntityInfo.id matches");

    let non_matching = query(
        &topology,
        &format!("type={}&idPattern={}", encode(&vtype), encode(&format!("{}.*", topology.id("Nowhere")))),
    )
    .await;
    assert!(!contains(&non_matching, &id), "a queried idPattern disjoint from EntityInfo.id does not match");
    topology.finish().await.expect("topology cleanup");
}

/// Id sub-case: when both the queried idPattern and EntityInfo.idPattern are
/// present, overlapping patterns match conservatively (clause 5.12). Confirms
/// REQ-DISC-106.
#[tokio::test]
async fn matching_id_subcase_both_patterns_present_conservative_match() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("BothPatType");
    let id = topology.id("registration:both-patterns");
    register(
        &topology,
        &CsrBuilder::new(&id, PEER_ENDPOINT)
            .entity_type(&vtype)
            .id_pattern(&format!("{}.*", topology.id("Fleet")))
            .build(),
    )
    .await;

    let overlapping = query(
        &topology,
        &format!("type={}&idPattern={}", encode(&vtype), encode(&format!("{}.*", topology.id("Fleet")))),
    )
    .await;
    assert!(
        contains(&overlapping, &id),
        "overlapping query and registration idPatterns match conservatively"
    );
    topology.finish().await.expect("topology cleanup");
}
