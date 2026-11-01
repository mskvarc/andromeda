//! The claim columns of the claimed queues.
//!
//! A claim is the worker that holds it and the instant its lease lapses, and
//! nothing reads when it was taken, so each queue holds exactly `claimed_by`
//! and `lease_until`, set and cleared together.

use crate::repository::{PinError, constraint_definition, schema};
use andromeda_sql::declared_schema::column::DeclaredColumn;
use pretty_assertions::assert_eq;

/// The claimed queues and the check that keeps each one's claim whole.
const CLAIM_COHERENCE: [(&str, &str); 5] = [
    ("notification_deliveries", "chk_notification_deliveries_claim_coherent"),
    ("pending_side_effects", "chk_pse_claim_coherent"),
    ("temporal_append_intents", "chk_temporal_append_intents_claim_coherent"),
    ("snapshot_notification_outbox", "chk_snapshot_notification_outbox_claim_coherent"),
    ("snapshot_preparation", "chk_snapshot_preparation_claim_coherent"),
];

#[test]
fn no_relation_records_when_a_claim_was_taken() -> Result<(), PinError> {
    let schema = schema()?;
    let claimed_at: Vec<&str> = schema
        .columns()
        .iter()
        .filter(|column| column.name() == "claimed_at")
        .map(DeclaredColumn::relation)
        .collect();
    assert_eq!(claimed_at, Vec::<&str>::new());
    Ok(())
}

#[test]
fn every_claimed_queue_sets_and_clears_its_worker_and_its_lease_together() -> Result<(), PinError> {
    let schema = schema()?;
    for (table, name) in CLAIM_COHERENCE {
        let definition = constraint_definition(&schema, name)?;
        assert!(definition.contains("num_nonnulls(claimed_by, lease_until) in (0, 2)"), "{table}: {definition}");
    }
    Ok(())
}
