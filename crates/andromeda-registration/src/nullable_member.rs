//! The three states a nullable member of a registration body can be in.
//!
//! Clause 5.5.8 gives an update fragment member-removal semantics: a member the
//! fragment omits leaves the stored value alone, a member carried as `null`
//! removes the stored value, and a member carrying a value replaces it. Those
//! are three distinct outcomes, so they are three variants rather than a nested
//! `Option` whose two layers would silently mean different things. A creation
//! body reads its members the same way and refuses the one that is `null`.

use crate::{csr_member::CsrMember, error::RegistrationError, registration_validation_reason::RegistrationValidationReason};
use serde::{Deserialize, Deserializer};

/// One nullable member of a registration body (clause 5.5.8).
#[derive(Debug, PartialEq, Eq)]
pub enum NullableMember<T> {
    /// The body does not carry the member; the stored value stands.
    Absent,
    /// The body carries the member as `null`; the stored value is removed.
    Cleared,
    /// The body carries a replacement value for the member.
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

/// Hand-written because the three states are told apart by the member's
/// presence, which a deserializer reports by never calling this at all, and by
/// its `null`, which no derive maps onto a variant.
impl<'de, T: Deserialize<'de>> Deserialize<'de> for NullableMember<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<NullableMember<T>, D::Error> {
        Ok(Option::<T>::deserialize(deserializer)?.map_or(NullableMember::Cleared, NullableMember::Set))
    }
}

impl<T> NullableMember<T> {
    /// Whether the body leaves the member out.
    #[must_use]
    pub const fn is_absent(&self) -> bool {
        matches!(self, NullableMember::Absent)
    }

    /// The replacement value the member carries, borrowed mutably; nothing for
    /// a member the body omits or removes.
    pub const fn as_set_mut(&mut self) -> Option<&mut T> {
        match self {
            NullableMember::Set(value) => Some(value),
            NullableMember::Absent | NullableMember::Cleared => None,
        }
    }

    /// The value of a member that may not be removed.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationValidationReason::MemberMustNotBeNull`] naming
    /// `member` when the body carries it as `null`.
    pub fn present(self, member: CsrMember) -> Result<Option<T>, RegistrationError> {
        match self {
            NullableMember::Absent => Ok(None),
            NullableMember::Set(value) => Ok(Some(value)),
            NullableMember::Cleared => Err(RegistrationError::Validation(RegistrationValidationReason::MemberMustNotBeNull { member })),
        }
    }

    /// The member in the same state, its carried value converted by `convert`.
    #[must_use]
    pub fn map<U>(self, convert: impl FnOnce(T) -> U) -> NullableMember<U> {
        match self {
            NullableMember::Absent => NullableMember::Absent,
            NullableMember::Cleared => NullableMember::Cleared,
            NullableMember::Set(value) => NullableMember::Set(convert(value)),
        }
    }

    /// The value of a member a creation body must carry (clause 5.2.9
    /// cardinality 1).
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationValidationReason::MemberRequired`] when the body
    /// omits the member, and the refusal of [`NullableMember::present`] when it
    /// carries it as `null`.
    pub fn required(self, member: CsrMember) -> Result<T, RegistrationError> {
        self.present(member)?
            .ok_or(RegistrationError::Validation(RegistrationValidationReason::MemberRequired { member }))
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
    use crate::{
        csr_member::CsrMember,
        error::RegistrationError,
        nullable_member::NullableMember,
        registration_validation_reason::RegistrationValidationReason,
    };

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
    fn mapping_a_member_keeps_its_state_and_converts_its_value() {
        pretty_assertions::assert_eq!(NullableMember::<u8>::Absent.map(u16::from), NullableMember::Absent);
        pretty_assertions::assert_eq!(NullableMember::<u8>::Cleared.map(u16::from), NullableMember::Cleared);
        pretty_assertions::assert_eq!(NullableMember::Set(3_u8).map(u16::from), NullableMember::Set(3_u16));
    }

    #[test]
    fn the_default_member_is_absent() {
        pretty_assertions::assert_eq!(NullableMember::<u8>::default(), NullableMember::Absent);
    }

    #[test]
    fn only_a_set_member_lends_its_value() {
        pretty_assertions::assert_eq!(NullableMember::Set(1_u8).as_set_mut().copied(), Some(1));
        pretty_assertions::assert_eq!(NullableMember::<u8>::Absent.as_set_mut(), None);
        pretty_assertions::assert_eq!(NullableMember::<u8>::Cleared.as_set_mut(), None);
    }

    #[test]
    fn a_member_reads_as_absent_cleared_or_set_from_its_json() {
        let read = |text: &str| serde_json::from_str::<NullableMember<u8>>(text).expect("a JSON member");
        pretty_assertions::assert_eq!(read("null"), NullableMember::Cleared);
        pretty_assertions::assert_eq!(read("5"), NullableMember::Set(5));
        assert!(serde_json::from_str::<NullableMember<u8>>(r#""five""#).is_err());
    }

    #[test]
    fn a_member_that_may_not_be_removed_refuses_null_and_keeps_the_rest() {
        pretty_assertions::assert_eq!(NullableMember::<u8>::Absent.present(CsrMember::Mode).expect("absent is allowed"), None);
        pretty_assertions::assert_eq!(NullableMember::Set(3_u8).present(CsrMember::Mode).expect("a value is allowed"), Some(3));
        let refusal = NullableMember::<u8>::Cleared.present(CsrMember::Mode).expect_err("null is refused");
        assert!(
            matches!(
                refusal,
                RegistrationError::Validation(RegistrationValidationReason::MemberMustNotBeNull { member: CsrMember::Mode })
            ),
            "{refusal:?}"
        );
        pretty_assertions::assert_eq!(refusal.to_string(), "invalid context-source registration: mode must not be null");
    }

    #[test]
    fn a_required_member_must_be_carried_with_a_value() {
        pretty_assertions::assert_eq!(NullableMember::Set(3_u8).required(CsrMember::Endpoint).expect("a value"), 3);
        let missing = NullableMember::<u8>::Absent.required(CsrMember::Endpoint).expect_err("absent is refused");
        assert!(
            matches!(
                missing,
                RegistrationError::Validation(RegistrationValidationReason::MemberRequired { member: CsrMember::Endpoint })
            ),
            "{missing:?}"
        );
        assert!(NullableMember::<u8>::Cleared.required(CsrMember::Endpoint).is_err());
    }
}
