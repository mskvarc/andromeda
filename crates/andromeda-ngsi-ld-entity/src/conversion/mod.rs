//! Turning what a write carries into what an entity is, and an entity back
//! into the document it is.
//!
//! The conversions into state refuse everything state cannot hold, with the
//! refusal a create or a replacement answers; the conversions out of state
//! cannot fail, since every state is a document a write could have carried.

pub mod document_from_state;
pub mod entity_from_document;
pub mod fragment_value;
pub mod instance_from_document;
pub mod stored_value;
