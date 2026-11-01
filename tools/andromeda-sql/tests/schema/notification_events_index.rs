//! The index the orphan-event sweep reads `notification_events` through.
//!
//! The sweep removes the events older than its cutoff that no delivery
//! references, a bound on `created_at` alone. Events are written in creation
//! order and never updated, so a block range summary of the column narrows the
//! sweep to the oldest pages for an index a small fraction of the heap.

use crate::repository::{PinError, index_definition, schema};

#[test]
fn the_orphan_event_sweep_reads_a_block_range_summary_of_the_creation_instant() -> Result<(), PinError> {
    let schema = schema()?;
    let definition = index_definition(&schema, "idx_notification_events_created_at")?;
    assert!(
        definition.ends_with("on notification_events using brin (created_at) with (pages_per_range = 32, autosummarize = on)"),
        "{definition}"
    );
    Ok(())
}
