//! The three states a nullable member of a registration update fragment can be
//! in.
//!
//! Clause 5.5.8 gives an update fragment member-removal semantics: a member the
//! fragment omits leaves the stored value alone, a member carried as `null`
//! removes the stored value, and a member carrying a value replaces it. Those
//! are three distinct outcomes, so they are three variants rather than a nested
//! `Option` whose two layers would silently mean different things.

/// One nullable member of a registration update fragment (clause 5.5.8).
#[derive(Debug, PartialEq, Eq)]
pub enum NullableMember<T> {
    /// The fragment does not carry the member; the stored value stands.
    Absent,
    /// The fragment carries the member as `null`; the stored value is removed.
    Cleared,
    /// The fragment carries a replacement value for the member.
    Set(T),
}

/// Hand-written because `#[derive(Default)]` bounds every type parameter on
/// `Default`, and members such as `Url` and `MediaTypeBuf` have no default
/// value at all. The absent state holds no `T`, so it needs none.
impl<T> Default for NullableMember<T> {
    fn default() -> NullableMember<T> {
        NullableMember::Absent
    }
}

impl<T: Clone> NullableMember<T> {
    /// Resolve this member against the value the stored registration carries,
    /// yielding the value that a fully-resolved update writes.
    #[must_use]
    pub fn resolve(&self, stored: Option<T>) -> Option<T> {
        match self {
            NullableMember::Absent => stored,
            NullableMember::Cleared => None,
            // clone: the fragment is borrowed and the resolved update owns every value it writes.
            NullableMember::Set(value) => Some(value.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::nullable_member::NullableMember;

    #[test]
    fn an_absent_member_keeps_the_stored_value() {
        let member: NullableMember<u8> = NullableMember::Absent;
        pretty_assertions::assert_eq!(member.resolve(Some(7)), Some(7));
    }

    #[test]
    fn a_cleared_member_removes_the_stored_value() {
        let member: NullableMember<u8> = NullableMember::Cleared;
        pretty_assertions::assert_eq!(member.resolve(Some(7)), None);
    }

    #[test]
    fn a_set_member_replaces_the_stored_value() {
        let member: NullableMember<u8> = NullableMember::Set(9);
        pretty_assertions::assert_eq!(member.resolve(Some(7)), Some(9));
    }

    #[test]
    fn the_default_member_is_absent() {
        pretty_assertions::assert_eq!(NullableMember::<u8>::default(), NullableMember::Absent);
    }
}
