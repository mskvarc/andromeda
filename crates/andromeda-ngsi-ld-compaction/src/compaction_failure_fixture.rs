//! Compaction failures of stored state, for tests that classify one without a
//! store or a JSON-LD processor.

use crate::compaction_error::CompactionError;
use andromeda_ngsi_ld_model::node_refusal::{NodeRefusal, RefusedNode};
use andromeda_ngsi_ld_number::json_value::json_pointer::JsonPointer;
use serde::ser::Error as _;

/// The compact model refusing a stored entity at its top, where a node that is
/// no JSON object stands.
#[must_use]
pub const fn refused_stored_entity() -> CompactionError {
    CompactionError::Refused(RefusedNode {
        path: JsonPointer::from_segments(Vec::new()),
        refusal: NodeRefusal::NotAnObject,
    })
}

/// A stored entity that could not be written in its expanded form for the
/// processor, the failure its serialisation reports.
#[must_use]
pub fn unencodable_stored_entity() -> CompactionError {
    CompactionError::Encode(serde_json::Error::custom("the stored entity could not be serialised"))
}
