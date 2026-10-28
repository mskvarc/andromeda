//! The need codes a queued temporal intent may store.

use crate::repository::{PinError, constraint_definition, schema};
use andromeda_postgresql_codes::temporal_intent_reach::TemporalIntentReach;
use pretty_assertions::assert_eq;

/// The check admits exactly the codes the registry assigns, so a need added to
/// the registry without a migration widening the check fails here rather than
/// as a constraint violation on the first intent that meets it.
#[test]
fn the_entry_needs_check_admits_every_registered_need_code() -> Result<(), PinError> {
    let schema = schema()?;
    let codes: Vec<i16> = TemporalIntentReach::ALL.iter().map(|reach| reach.code()).collect();
    let (Some(lowest), Some(highest)) = (codes.iter().min(), codes.iter().max()) else {
        return Err(PinError::Missing("a registered need code".to_owned()));
    };
    assert_eq!(codes, (*lowest..=*highest).collect::<Vec<_>>(), "the codes are contiguous");
    assert_eq!(
        constraint_definition(&schema, "chk_temporal_append_intents_entry_needs")?,
        format!("check (array_position(entry_needs, null) is null and {lowest} <= all (entry_needs) and {highest} >= all (entry_needs))")
    );
    Ok(())
}
