//! A seal no write admits is dropped: a `created` clause 4.6.3 does not
//! spell, subproperties that do not name an entity, a seal that is no
//! Property, and a value that is no single complete proof. Each seal signs
//! the one attribute of an Entity of its own, beside a complete seal that is
//! kept.

use crate::fixtures::{
    current_state::{insert_entity, insert_number_attribute},
    observed::current_seals_as_the_broker_reads,
    rehearsal::{at_v1_0_0, intern_seal_attribute},
    statements::read_documents,
    test_error::UpgradeTestError,
    vocabulary::{
        ENTITY_ID_SEALED,
        ENTITY_TYPE_SEALED,
        PROOF_CREATED,
        SEALED_ENTITY_ID,
        SEALED_TYPE,
        complete_seals,
        proof_node,
        proof_node_with,
        seal_element,
        seals,
        seals_naming,
    },
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// The expanded `created` member name.
const CREATED: &str = "http://purl.org/dc/terms/created";

/// The datatype the W3C Data Integrity v2 context gives `created`.
const DATE_TIME: &str = "http://www.w3.org/2001/XMLSchema#dateTime";

/// The expanded `cryptosuite` member name.
const CRYPTOSUITE: &str = "https://w3id.org/security#cryptosuite";

/// A proof whose `created` is written as `text`.
fn created(text: &str) -> Value {
    proof_node_with(CREATED, json!([{"@type": DATE_TIME, "@value": text}]))
}

/// What each refused seal is and the proof and subproperties it holds.
fn refused_seals() -> Vec<(&'static str, Value, Value)> {
    let whole = complete_seals;
    vec![
        ("a seventh fractional digit", created("2025-01-27T21:02:24.1234567Z"), whole()),
        ("a numeric offset", created("2025-01-27T21:02:24+01:00"), whole()),
        ("lowercase designators", created("2025-01-27t21:02:24z"), whole()),
        ("a day its month lacks", created("2025-02-30T00:00:00Z"), whole()),
        ("a leap day in a common year", created("2025-02-29T00:00:00Z"), whole()),
        ("hour 24", created("2025-01-27T24:00:00Z"), whole()),
        ("second 60", created("2025-01-27T21:02:60Z"), whole()),
        ("no seconds", created("2025-01-27T21:02Z"), whole()),
        (
            "a created of no datatype",
            proof_node_with(CREATED, json!([{"@value": PROOF_CREATED}])),
            whole(),
        ),
        (
            "an empty cryptosuite",
            proof_node_with(CRYPTOSUITE, json!([{"@type": "https://w3id.org/security#cryptosuiteString", "@value": ""}])),
            whole(),
        ),
        ("two proofs", json!([proof_node(), proof_node()]), whole()),
        ("no entityTypeSealed", proof_node(), json!({ ENTITY_ID_SEALED: [{"@value": SEALED_ENTITY_ID}] })),
        ("no entityIdSealed", proof_node(), json!({ ENTITY_TYPE_SEALED: [{"@id": SEALED_TYPE}] })),
        ("no type named", proof_node(), seals(&json!([]))),
        (
            "the NGSI-LD Null as the entity",
            proof_node(),
            seals_naming("urn:ngsi-ld:null", &json!([{"@id": SEALED_TYPE}])),
        ),
        ("the NGSI-LD Null as a type", proof_node(), seals(&json!([{"@id": "urn:ngsi-ld:null"}]))),
        (
            "an entity that is no URI",
            proof_node(),
            seals_naming("Store 002", &json!([{"@id": SEALED_TYPE}])),
        ),
        (
            "an entity named by reference",
            proof_node(),
            json!({ ENTITY_ID_SEALED: [{"@id": SEALED_ENTITY_ID}], ENTITY_TYPE_SEALED: [{"@id": SEALED_TYPE}] }),
        ),
        (
            "an entity of 1024 bytes",
            proof_node(),
            seals_naming(&format!("urn:ngsi-ld:Store:{}", "a".repeat(1006)), &json!([{"@id": SEALED_TYPE}])),
        ),
    ]
}

#[tokio::test]
async fn a_seal_no_write_admits_is_dropped_beside_a_complete_one_that_is_kept() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let refused = refused_seals();
    for (position, (_, proof, subproperties)) in refused.iter().enumerate() {
        let entity_key = insert_entity(database.pool(), &format!("urn:ngsi-ld:Store:{position}")).await?;
        let seal = seal_element(0, seal_key, proof, subproperties);
        insert_number_attribute(database.pool(), entity_key, keys.temperature, 0, &[seal]).await?;
    }
    let mut relationship = seal_element(0, seal_key, &proof_node(), &complete_seals());
    relationship["attribute_kind"] = json!(2);
    let related = insert_entity(database.pool(), "urn:ngsi-ld:Store:related").await?;
    insert_number_attribute(database.pool(), related, keys.temperature, 0, &[relationship]).await?;
    let kept = insert_entity(database.pool(), "urn:ngsi-ld:Store:kept").await?;
    let complete = seal_element(0, seal_key, &proof_node(), &complete_seals());
    insert_number_attribute(database.pool(), kept, keys.temperature, 0, &[complete]).await?;

    database.migrate_to_current().await?;

    let rows = read_documents(database.pool(), "current_attributes", "sub_attributes", "entity_key", None).await?;
    assert_eq!(rows.len(), refused.len() + 2);
    for ((what, _, _), left) in refused.iter().zip(&rows) {
        assert_eq!(left, &None, "a seal with {what} is dropped");
    }
    assert_eq!(rows[refused.len()], None, "a seal that is a Relationship is dropped");
    assert!(rows[refused.len() + 1].is_some(), "the complete seal is kept");
    assert_eq!(current_seals_as_the_broker_reads(database.pool()).await?.len(), 1);
    database.drop_database().await?;
    Ok(())
}
