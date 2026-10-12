//! A member a write either sets or deletes.
//!
//! Clause 4.5.0 makes the NGSI-LD Null "the removal of a target member" in a
//! partial update (clause 5.5.8) or a merge (clause 5.5.12). A written member is
//! therefore one of two things, never a third.

/// A member a written fragment sets or removes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Slot<T> {
    /// The member is set to this value.
    Set(T),
    /// The member is the NGSI-LD Null: remove it.
    Remove,
}

impl<T> Slot<T> {
    /// The value, when the member is set.
    #[must_use]
    pub const fn as_set(&self) -> Option<&T> {
        match self {
            Slot::Set(value) => Some(value),
            Slot::Remove => None,
        }
    }

    /// The value, when the member is set, given up.
    #[must_use]
    pub fn into_set(self) -> Option<T> {
        match self {
            Slot::Set(value) => Some(value),
            Slot::Remove => None,
        }
    }

    /// Whether the member is the NGSI-LD Null.
    #[must_use]
    pub const fn is_remove(&self) -> bool {
        matches!(self, Slot::Remove)
    }
}
