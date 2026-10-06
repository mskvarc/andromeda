//! A member an update fragment may leave out, clear or set.
//!
//! Clause 5.8.2.4 updates a Subscription with a fragment "as mandated by clause
//! 5.5.8": a member the fragment does not mention keeps its stored value, an
//! explicit `null` (the NGSI-LD Null) removes it, and any other value replaces
//! it. Where a member takes a URI, clause 4.5.0 spells the Null as the URI
//! `urn:ngsi-ld:null` too, and clause 5.2.1 refuses an NGSI-LD Null anywhere
//! but in an update (clauses 5.5.8 and 5.5.12), a JSON `null` on creation
//! included. Four states, so neither an `Option` (which cannot say
//! "cleared") nor a pair of them (which hides the other states in a nesting)
//! names them.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A member of an update fragment, as the fragment wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clearable<T> {
    /// The fragment does not mention the member, which keeps its stored value.
    Absent,
    /// The fragment wrote `null`, which removes the member from a stored one.
    Cleared,
    /// The fragment wrote the NGSI-LD Null as the URI `urn:ngsi-ld:null` (clause
    /// 4.5.0), which removes the member from a stored one.
    NgsiLdNull,
    /// The fragment wrote a value, which replaces the stored one.
    Set(T),
}

impl<T> Clearable<T> {
    /// The member a fragment does not mention.
    #[must_use]
    pub const fn absent() -> Clearable<T> {
        Clearable::Absent
    }

    /// Whether the fragment does not mention the member.
    #[must_use]
    pub const fn is_absent(&self) -> bool {
        matches!(self, Clearable::Absent)
    }

    /// The member of a body that names a value or leaves the member out, which
    /// is how a stored subscription is written back as a request.
    #[must_use]
    pub fn from_option(value: Option<T>) -> Clearable<T> {
        value.map_or(Clearable::Absent, Clearable::Set)
    }

    /// Whether the fragment wrote the NGSI-LD Null as a URI.
    #[must_use]
    pub const fn is_ngsi_ld_null(&self) -> bool {
        matches!(self, Clearable::NgsiLdNull)
    }

    /// Whether the fragment wrote the member as a null of either spelling, a
    /// JSON `null` or the NGSI-LD Null URI, which clause 5.2.1 refuses outside
    /// an update.
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Clearable::Cleared | Clearable::NgsiLdNull)
    }

    /// The value the fragment wrote, if it wrote one.
    #[must_use]
    pub const fn as_set(&self) -> Option<&T> {
        match self {
            Clearable::Set(value) => Some(value),
            Clearable::Absent | Clearable::Cleared | Clearable::NgsiLdNull => None,
        }
    }

    /// The value the fragment wrote, if it wrote one, for the caller to change.
    pub const fn as_set_mut(&mut self) -> Option<&mut T> {
        match self {
            Clearable::Set(value) => Some(value),
            Clearable::Absent | Clearable::Cleared | Clearable::NgsiLdNull => None,
        }
    }

    /// The member a create body holds: a create has nothing stored, so a
    /// member it leaves out is absent. A create that wrote the member as a
    /// null of either spelling is refused before this is read (clause 5.2.1).
    #[must_use]
    pub fn into_created(self) -> Option<T> {
        match self {
            Clearable::Set(value) => Some(value),
            Clearable::Absent | Clearable::Cleared | Clearable::NgsiLdNull => None,
        }
    }

    /// The member after the fragment is applied over the `stored` one.
    #[must_use]
    pub fn applied_over(self, stored: Option<T>) -> Option<T> {
        match self {
            Clearable::Absent => stored,
            Clearable::Cleared | Clearable::NgsiLdNull => None,
            Clearable::Set(value) => Some(value),
        }
    }
}

// A derive would bound `T: Default`, which a member such as a `GeoQuery` is not,
// while the absent member needs no value.
impl<T> Default for Clearable<T> {
    fn default() -> Clearable<T> {
        Clearable::absent()
    }
}

// A derive cannot read `null` as `Cleared`: it is the member's own `null`, which
// the carrier field's `#[serde(default)]` leaves distinct from an absent member.
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Clearable<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Clearable<T>, D::Error> {
        Ok(Option::<T>::deserialize(deserializer)?.map_or(Clearable::Cleared, Clearable::Set))
    }
}

// A derive cannot write `Cleared` as `null` and leave `Absent` unwritten; a
// carrier field skips an absent member with `skip_serializing_if`. The NGSI-LD
// Null is written as `null` as well: both delete the member of a stored
// subscription, and the one request that tells them apart is a create, which
// refuses the URI before anything is written.
impl<T: Serialize> Serialize for Clearable<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Clearable::Set(value) => value.serialize(serializer),
            Clearable::Absent | Clearable::Cleared | Clearable::NgsiLdNull => serializer.serialize_none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::clearable::Clearable;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    /// A carrier standing in for an update fragment.
    #[derive(Debug, Serialize, Deserialize)]
    struct Fragment {
        #[serde(default, skip_serializing_if = "Clearable::is_absent")]
        member: Clearable<Vec<u8>>,
    }

    #[test]
    fn a_member_is_told_absent_from_cleared_from_set() {
        let absent: Fragment = serde_json::from_value(json!({})).expect("absent");
        pretty_assertions::assert_eq!(absent.member, Clearable::Absent);
        let cleared: Fragment = serde_json::from_value(json!({"member": null})).expect("null");
        pretty_assertions::assert_eq!(cleared.member, Clearable::Cleared);
        let set: Fragment = serde_json::from_value(json!({"member": [1, 2]})).expect("a value");
        pretty_assertions::assert_eq!(set.member, Clearable::Set(vec![1, 2]));
        pretty_assertions::assert_eq!(serde_json::to_value(&absent).expect("writes"), json!({}));
        pretty_assertions::assert_eq!(serde_json::to_value(&cleared).expect("writes"), json!({"member": null}));
        pretty_assertions::assert_eq!(serde_json::to_value(&set).expect("writes"), json!({"member": [1, 2]}));
    }

    /// Clause 5.8.3: applied over a stored member, an absent one keeps it, a
    /// cleared one removes it and a set one replaces it.
    #[test]
    fn a_fragment_member_is_applied_over_the_stored_one() {
        pretty_assertions::assert_eq!(Clearable::Absent.applied_over(Some(1)), Some(1));
        pretty_assertions::assert_eq!(Clearable::<u8>::Cleared.applied_over(Some(1)), None);
        pretty_assertions::assert_eq!(Clearable::<u8>::NgsiLdNull.applied_over(Some(1)), None);
        pretty_assertions::assert_eq!(Clearable::Set(2).applied_over(Some(1)), Some(2));
        pretty_assertions::assert_eq!(Clearable::Set(2).applied_over(None), Some(2));
    }

    /// A create has nothing stored, so only a set member survives.
    #[test]
    fn a_created_member_is_present_only_when_set() {
        pretty_assertions::assert_eq!(Clearable::<u8>::Absent.into_created(), None);
        pretty_assertions::assert_eq!(Clearable::<u8>::Cleared.into_created(), None);
        pretty_assertions::assert_eq!(Clearable::<u8>::NgsiLdNull.into_created(), None);
        assert!(Clearable::<u8>::NgsiLdNull.is_ngsi_ld_null());
        assert!(!Clearable::<u8>::Cleared.is_ngsi_ld_null());
    }

    /// Clause 5.2.1: both spellings of a null are told from a member that is
    /// absent or set.
    #[test]
    fn a_null_of_either_spelling_is_told_from_an_absent_or_a_set_member() {
        assert!(Clearable::<u8>::Cleared.is_null());
        assert!(Clearable::<u8>::NgsiLdNull.is_null());
        assert!(!Clearable::<u8>::Absent.is_null());
        assert!(!Clearable::Set(1).is_null());
    }

    #[test]
    fn a_created_member_that_is_set_is_kept() {
        pretty_assertions::assert_eq!(Clearable::Set(3).into_created(), Some(3));
    }
}
