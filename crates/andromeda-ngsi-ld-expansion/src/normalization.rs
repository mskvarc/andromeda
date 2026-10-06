//! What a first-level NGSI-LD Null member of an entity fragment means before
//! expansion.
//!
//! A request or a notification may write the NGSI-LD Null in place of a whole
//! attribute member. Below the top level it is always the value of a concise
//! Property (clause 4.5.2.3); at the top level what the entity is decides, because a
//! merge (clause 5.5.12) and a notification (clause 5.8.6) keep a first-level
//! NGSI-LD Null member as the deletion of the whole member rather than reading
//! it as a concise Property value.

/// What a first-level NGSI-LD Null member of an entity fragment is.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Normalization {
    /// A first-level Null member is the value of a concise Property (create,
    /// replace, append, update, upsert).
    #[default]
    Entity,
    /// A first-level NGSI-LD Null member stays as written, the deletion of the
    /// whole member: a merge removes the member by it (clause 5.5.12), and a
    /// notification reports the member deleted by it (clause 5.8.6).
    MemberDeletions,
}
