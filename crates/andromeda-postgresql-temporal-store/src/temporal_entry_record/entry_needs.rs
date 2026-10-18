//! Which application statements the entries of one intent reach.
//!
//! An intent records the codes of the needs its entries meet, so an
//! application skips every statement none of its intents reaches. Only an
//! attribute entry carries an instance identity: an instance of the Scope has
//! none (clause 4.5.6).

use crate::temporal_entry_record::TemporalEntryRecord;
use andromeda_postgresql_codes::{temporal_entry_kind::TemporalEntryKind, temporal_intent_reach::TemporalIntentReach};

/// The entry codes a Scope entry is stored under: an event, or an assertion
/// of the Scope in force.
const SCOPE_KINDS: [i16; 2] = [TemporalEntryKind::Scope.code(), TemporalEntryKind::ScopeAssertion.code()];

/// Whether an intent stating `entity_type_name_keys` and holding `entries`
/// meets `need`.
///
/// An intent observes when it holds no entry or any entry that does not end
/// an instance: clause 4.5.1 requires only `id` and `type` of an Entity, so a
/// write observing one with no attribute still extends its evolution.
pub(super) fn meets(entity_type_name_keys: &[i32], entries: &[TemporalEntryRecord], need: TemporalIntentReach) -> bool {
    let is_attribute = |entry: &TemporalEntryRecord| !SCOPE_KINDS.contains(&entry.entry_kind);
    match need {
        TemporalIntentReach::TypedIntent => !entity_type_name_keys.is_empty(),
        TemporalIntentReach::UntypedObservation => {
            entity_type_name_keys.is_empty() && (entries.is_empty() || entries.iter().any(|entry| entry.entry_kind != TemporalEntryKind::Tombstone.code()))
        }
        TemporalIntentReach::AttributeEntry => entries.iter().any(is_attribute),
        TemporalIntentReach::VerbatimInstance => entries.iter().any(|entry| entry.verbatim_instance_id.is_some()),
        TemporalIntentReach::ScopeEntry => !entries.iter().all(is_attribute),
        TemporalIntentReach::UntypedAttributeEntry => entity_type_name_keys.is_empty() && entries.iter().any(is_attribute),
    }
}
