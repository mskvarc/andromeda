//! Seals at the edges of what the broker accepts are kept, and one just past
//! an edge is dropped: a URL's port, the earliest `created` a `DateTime` can
//! spell, a comma before its fraction, the characters of an IRI and the
//! length of an identifier. Every kept seal is read back through the broker's
//! own decoder.

use crate::fixtures::{
    current_state::{insert_entity, insert_number_attribute},
    observed::{current_proofs_as_written, current_seals_as_the_broker_reads, current_sub_attributes},
    rehearsal::{at_v1_0_0, intern_seal_attribute},
    test_error::UpgradeTestError,
    vocabulary::{SEALED_ENTITY_ID, SEALED_TYPE, proof_node, proof_node_with, seal_element, seals_naming},
};
use andromeda_postgresql_attribute_instance::stored_proof::StoredProof;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// The expanded `verificationMethod` member name.
const VERIFICATION_METHOD: &str = "https://w3id.org/security#verificationMethod";

/// The expanded `created` member name.
const CREATED: &str = "http://purl.org/dc/terms/created";

/// The datatype the W3C Data Integrity v2 context gives `created`.
const DATE_TIME: &str = "http://www.w3.org/2001/XMLSchema#dateTime";

/// The first and the last character of each of the first three ranges of
/// `ucschar` (IETF RFC 3987 section 2.2).
const UCSCHAR_BOUNDS: &str = "\u{a0}\u{d7ff}\u{f900}\u{fdcf}\u{fdf0}\u{ffef}";

/// A noncharacter between the second and the third range of `ucschar`.
const BETWEEN_UCSCHAR_RANGES: &str = "\u{fdd0}";

/// What the upgrade left of one seal.
struct Upgraded {
    /// The sub-attributes of the attribute the seal signed, `None` once the
    /// seal is dropped and nothing is left.
    sub_attributes: Option<Value>,
    /// The proof of every typed seal, as the model writes it.
    proofs: Vec<Value>,
    /// Every typed seal, as the broker reads it.
    read: Vec<StoredProof>,
}

/// Upgrades a database whose one temperature carries one seal of `proof`
/// naming the entity `entity_id`.
async fn upgrade_one_seal(proof: &Value, entity_id: &str) -> Result<Upgraded, UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let entity_key = insert_entity(database.pool(), "urn:ngsi-ld:Store:001").await?;
    let subproperties = seals_naming(entity_id, &json!([{"@id": SEALED_TYPE}]));
    let seal = seal_element(0, seal_key, proof, &subproperties);
    insert_number_attribute(database.pool(), entity_key, keys.temperature, 0, &[seal]).await?;

    database.migrate_to_current().await?;

    let upgraded = Upgraded {
        sub_attributes: current_sub_attributes(database.pool(), keys.temperature).await?,
        proofs: current_proofs_as_written(database.pool()).await?,
        read: current_seals_as_the_broker_reads(database.pool()).await?,
    };
    database.drop_database().await?;
    Ok(upgraded)
}

/// [`proof_node`] verified by the method at `url`.
fn verified_at(url: &str) -> Value {
    proof_node_with(VERIFICATION_METHOD, json!([{"@id": url}]))
}

/// The WHATWG URL Standard reads a port of any number of digits up to 65535,
/// leading zeros included, and refuses one past it.
#[tokio::test]
async fn a_seal_naming_a_port_a_url_can_carry_is_kept_and_one_past_it_is_dropped() -> Result<(), UpgradeTestError> {
    for url in ["https://example.org:65535/keys/1#key-1", "https://example.org:0065535/keys/1#key-1"] {
        let proof = verified_at(url);
        let upgraded = upgrade_one_seal(&proof, SEALED_ENTITY_ID).await?;
        assert_eq!(upgraded.proofs, [proof], "{url}");
        let methods: Vec<&str> = upgraded.read.iter().map(|seal| seal.verification_method.as_str()).collect();
        assert_eq!(methods, ["https://example.org:65535/keys/1#key-1"], "{url}");
    }
    let past = upgrade_one_seal(&verified_at("https://example.org:65536/keys/1#key-1"), SEALED_ENTITY_ID).await?;
    assert_eq!(past.sub_attributes, None);
    Ok(())
}

/// Clause 4.6.3 writes four year digits, so 0000 is the earliest year a
/// `DateTime` spells, and it is a leap year in the proleptic Gregorian
/// calendar of ISO 8601.
#[tokio::test]
async fn a_seal_created_on_the_leap_day_of_year_0000_keeps_its_created() -> Result<(), UpgradeTestError> {
    let proof = proof_node_with(CREATED, json!([{"@type": DATE_TIME, "@value": "0000-02-29T12:00:00Z"}]));
    let upgraded = upgrade_one_seal(&proof, SEALED_ENTITY_ID).await?;
    assert_eq!(upgraded.proofs, [proof]);
    let created: Vec<String> = upgraded.read.iter().map(|seal| seal.created.to_string()).collect();
    assert_eq!(created, ["0000-02-29T12:00:00Z"]);
    Ok(())
}

/// Clause 4.6.3 lets a request write a comma for the decimal point, and the
/// broker writes the point back.
#[tokio::test]
async fn a_seal_created_with_a_comma_before_its_fraction_reads_back_with_a_point() -> Result<(), UpgradeTestError> {
    let proof = proof_node_with(CREATED, json!([{"@type": DATE_TIME, "@value": "2025-01-27T21:02:24,50Z"}]));
    let upgraded = upgrade_one_seal(&proof, SEALED_ENTITY_ID).await?;
    let created: Vec<String> = upgraded.read.iter().map(|seal| seal.created.to_string()).collect();
    assert_eq!(created, ["2025-01-27T21:02:24.50Z"]);
    Ok(())
}

/// The broker adds the slash of an empty path to a URL, so the longest URL
/// with none that is an identifier is one byte below the identifier bound.
#[tokio::test]
async fn a_seal_naming_a_url_one_byte_below_the_identifier_bound_is_kept_and_one_at_it_is_dropped() -> Result<(), UpgradeTestError> {
    let below = format!("https://example.org?{}", "a".repeat(1003));
    assert_eq!(below.len(), 1023);
    let kept = upgrade_one_seal(&proof_node(), &below).await?;
    let entities: Vec<String> = kept.read.iter().map(|seal| seal.entity_id.to_string()).collect();
    assert_eq!(entities, [format!("https://example.org/?{}", "a".repeat(1003))]);
    let at = upgrade_one_seal(&proof_node(), &format!("{below}a")).await?;
    assert_eq!(at.sub_attributes, None);
    Ok(())
}

#[tokio::test]
async fn a_seal_naming_an_iri_at_the_bounds_of_ucschar_is_kept_and_one_between_its_ranges_is_dropped() -> Result<(), UpgradeTestError> {
    let at_bounds = format!("urn:ngsi-ld:Store:{UCSCHAR_BOUNDS}");
    let kept = upgrade_one_seal(&proof_node(), &at_bounds)
        .await?
        .sub_attributes
        .ok_or(UpgradeTestError::Missing { what: "the sub-attributes" })?;
    assert_eq!(kept[0]["seal_entity_id"], json!(at_bounds));
    let kept = upgrade_one_seal(&proof_node(), &at_bounds).await?;
    let entities: Vec<&str> = kept.read.iter().map(|seal| seal.entity_id.as_str()).collect();
    assert_eq!(entities, [at_bounds.as_str()]);
    let between = upgrade_one_seal(&proof_node(), &format!("urn:ngsi-ld:Store:{BETWEEN_UCSCHAR_RANGES}")).await?;
    assert_eq!(between.sub_attributes, None);
    Ok(())
}
