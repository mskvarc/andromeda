//! The `startAt` and `endAt` of a registration's time intervals are declared
//! as `DateTime` value objects in the stored Context Source Property document,
//! exactly as this build writes them, and nothing else in any document moves.

use crate::fixtures::{
    registrations::{insert_registration, stored_by_v1_0_0, written_document},
    rehearsal::at_v1_0_0,
    statements::read_documents,
    test_error::UpgradeTestError,
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// A registration declaring a closed `observationInterval`, whose `endAt`
/// carries a fraction, an open `managementInterval`, `expiresAt`, and an
/// added property holding an object with a `startAt` member of its own.
fn interval_registration() -> Value {
    json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:intervals",
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "https://example.org/Vehicle"}]}],
        "endpoint": "https://source.example.com/",
        "observationInterval": {"startAt": "2024-01-01T00:00:00Z", "endAt": "2025-01-01T00:00:00.5Z"},
        "managementInterval": {"startAt": "2024-02-01T00:00:00Z"},
        "expiresAt": "2030-12-31T23:59:59Z",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-02T00:00:00Z",
        "https://example.org/window": {"type": "Property", "value": {"startAt": "2024-01-01T00:00:00Z"}}
    })
}

/// A registration declaring no time interval.
fn plain_registration() -> Value {
    json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:plain",
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "https://example.org/Vehicle"}]}],
        "endpoint": "https://source.example.com/",
        "expiresAt": "2030-12-31T23:59:59Z",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-02T00:00:00Z"
    })
}

#[tokio::test]
async fn a_stored_time_interval_is_declared_as_this_build_writes_it_and_nothing_else_moves() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    let (intervals, plain) = (interval_registration(), plain_registration());
    let stored = stored_by_v1_0_0(&intervals)?;
    assert_ne!(stored, written_document(&intervals)?, "v1.0.0 wrote each interval otherwise");
    insert_registration(database.pool(), "urn:ngsi-ld:ContextSourceRegistration:intervals", &stored).await?;
    insert_registration(database.pool(), "urn:ngsi-ld:ContextSourceRegistration:plain", &stored_by_v1_0_0(&plain)?).await?;

    database.migrate_to_current().await?;

    let documents = read_documents(database.pool(), "context_source_registrations", "context_source_properties", "id", None).await?;
    assert_eq!(documents, vec![Some(written_document(&intervals)?), Some(written_document(&plain)?)]);
    database.drop_database().await?;
    Ok(())
}
