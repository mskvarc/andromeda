//! Seals at the edges of what the broker accepts are kept, and one just past
//! an edge is dropped: a URL's port, the earliest `created` a `DateTime` can
//! spell, and the characters of an IRI.

use crate::fixtures::{
    current_state::{insert_entity, insert_number_attribute},
    observed::{current_proofs_as_written, current_sub_attributes},
    rehearsal::{at_v1_0_0, intern_seal_attribute},
    test_error::UpgradeTestError,
    vocabulary::{SEALED_ENTITY_ID, SEALED_TYPE, proof_node, proof_node_with, seal_element, seals_naming},
};
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
        assert_eq!(upgrade_one_seal(&proof, SEALED_ENTITY_ID).await?.proofs, [proof], "{url}");
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
    assert_eq!(upgrade_one_seal(&proof, SEALED_ENTITY_ID).await?.proofs, [proof]);
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
    let between = upgrade_one_seal(&proof_node(), &format!("urn:ngsi-ld:Store:{BETWEEN_UCSCHAR_RANGES}")).await?;
    assert_eq!(between.sub_attributes, None);
    Ok(())
}
