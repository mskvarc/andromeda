//! The one pattern every migration-owned compact code domain is declared with.
//!
//! A compact code domain is a small, closed vocabulary that a migration writes
//! into a `SMALLINT` column on rows that outlive the release which wrote them.
//! Three properties make such a column safe to persist, and
//! [`compact_code_registry`](crate::compact_code_registry) is the single place
//! they are established rather than restated per domain.
//!
//! The number is written in the source, never chosen by the compiler. A Rust
//! enum discriminant that is left implicit moves when a member is inserted, and
//! a source-order position moves when a generator's input is reordered; neither
//! is a durable database contract, so every member carries its number as a
//! literal that matches the migration text.
//!
//! Assignment is append-only. A released number keeps naming what it named:
//! retired members stay decodable so historical rows keep their meaning, and a
//! new member takes the next unused number. The frozen-prefix digest below is
//! what turns that from a convention into a test: it covers the leading
//! registrations a released migration has already written to disk, so appending
//! beyond them is silent while altering, reordering or removing any of them
//! fails.
//!
//! An unrecognised number fails. Decoding is a [`TryFrom<i16>`] whose error is
//! [`UnknownCompactCode`]; nothing coerces an unknown number to a default,
//! because a row written by a newer release must be reported as unreadable
//! rather than silently reinterpreted as some other member.

use crate::compact_code_domain::CompactCodeDomain;
use thiserror::Error;

/// A stored number that the registry has never assigned.
///
/// Carries the registry it was rejected by, because a broker decodes several
/// code domains from one row and the number alone does not say which column was
/// unreadable.
#[derive(Clone, Copy, Debug, Eq, Error, Hash, PartialEq)]
#[error("{registry} has no member with code {code}")]
pub struct UnknownCompactCode {
    /// The registry that refused the number.
    pub registry: CompactCodeDomain,
    /// The number as it was read from storage.
    pub code: i16,
}

impl UnknownCompactCode {
    /// Report that `registry` has no member numbered `code`.
    #[must_use]
    pub const fn new(registry: CompactCodeDomain, code: i16) -> UnknownCompactCode {
        UnknownCompactCode { registry, code }
    }
}

/// The digest over the leading `prefix` registrations of a registry.
///
/// FNV-1a over each registration's two-byte number followed by its label, with
/// a separator between registrations so that moving a byte across the boundary
/// changes the result. It is deliberately computed from the registry's own
/// declaration and compared against a number frozen in that declaration: the
/// frozen number is the release's promise about what is already on disk, and
/// the recomputation is what detects a promise being broken.
///
/// A `prefix` beyond the registry's length digests the whole registry; the
/// generated stability test rejects that case separately, so this function
/// never has to fail.
#[must_use]
pub const fn frozen_prefix_digest(registrations: &[(i16, &str)], prefix: usize) -> u64 {
    let limit = if prefix < registrations.len() { prefix } else { registrations.len() };
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut index = 0;
    while index < limit {
        let number = registrations[index].0.to_be_bytes();
        hash = mix(hash, number[0]);
        hash = mix(hash, number[1]);
        let label = registrations[index].1.as_bytes();
        let mut offset = 0;
        while offset < label.len() {
            hash = mix(hash, label[offset]);
            offset += 1;
        }
        hash = mix(hash, RECORD_SEPARATOR);
        index += 1;
    }
    hash
}

/// The byte separating two registrations inside the digest.
const RECORD_SEPARATOR: u8 = 0x1e;

/// Fold one byte into an FNV-1a accumulator.
const fn mix(hash: u64, byte: u8) -> u64 {
    (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3)
}

/// Declare one migration-owned compact code domain.
///
/// Expands to the member enum with its numbers written as explicit `i16`
/// discriminants, the `ALL` slice of members in registration order, the
/// `REGISTRATIONS`, `FROZEN_PREFIX` and `FROZEN_DIGEST` constants that state
/// the domain's on-disk contract, a `code` accessor, a stable `label`
/// accessor, a [`TryFrom<i16>`] whose error is
/// [`UnknownCompactCode`], and a `#[cfg(test)]` module asserting the invariants
/// that make the numbers safe to persist: ascending unique numbers, unique
/// non-empty labels, a code round trip through every member, a typed failure
/// for an unassigned number, and the frozen-prefix digest.
///
/// The invoking module must have [`UnknownCompactCode`],
/// [`CompactCodeDomain`] and [`frozen_prefix_digest`] in scope, `$name` must
/// be a [`CompactCodeDomain`] variant, and may declare only one registry, since
/// the generated test module has a fixed name.
///
/// `frozen_prefix` is how many leading registrations a released migration has
/// already written to disk, and `frozen_digest` is
/// [`frozen_prefix_digest`] over exactly those. Appending a member leaves both
/// untouched; changing one of the frozen registrations fails the generated
/// test, and the failure message carries the digest that would have to be
/// blessed if the change is genuinely intended.
///
/// ```ignore
/// compact_code_registry! {
///     /// What the domain means.
///     ExampleKind {
///         frozen_prefix: 1,
///         frozen_digest: 0x0000_0000_0000_0000,
///         codes: {
///             /// What the member means.
///             First = 1, "first";
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! compact_code_registry {
    (
        $(#[$registry_doc:meta])*
        $name:ident {
            frozen_prefix: $frozen_prefix:literal,
            frozen_digest: $frozen_digest:literal,
            codes: {
                $(
                    $(#[$member_doc:meta])*
                    $member:ident = $code:literal, $label:literal;
                )+
            }
        }
    ) => {
        $(#[$registry_doc])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #[repr(i16)]
        pub enum $name {
            $(
                $(#[$member_doc])*
                $member = $code,
            )+
        }

        impl $name {
            /// Every member the registry has assigned, in registration order.
            pub const ALL: &'static [$name] = &[$($name::$member),+];

            /// The persisted number and stable label of every member.
            pub const REGISTRATIONS: &'static [(i16, &'static str)] = &[$(($code, $label)),+];

            /// How many leading registrations a released migration has written to disk.
            pub const FROZEN_PREFIX: usize = $frozen_prefix;

            /// The digest those frozen registrations were released at.
            pub const FROZEN_DIGEST: u64 = $frozen_digest;

            /// The number this registry persists for the member.
            pub const fn code(self) -> i16 {
                self as i16
            }

            /// The member's stable label.
            pub const fn label(self) -> &'static str {
                match self {
                    $($name::$member => $label,)+
                }
            }
        }

        impl TryFrom<i16> for $name {
            type Error = UnknownCompactCode;

            fn try_from(stored: i16) -> Result<$name, UnknownCompactCode> {
                match stored {
                    $($code => Ok($name::$member),)+
                    unassigned => Err(UnknownCompactCode::new(CompactCodeDomain::$name, unassigned)),
                }
            }
        }

        #[cfg(test)]
        /// The invariants that make this registry's numbers safe to persist.
        mod registry_stability {
            use pretty_assertions::assert_eq;
            use super::$name;
            use $crate::registry::frozen_prefix_digest;

            #[test]
            fn numbers_are_unique_and_ascend_in_registration_order() {
                for pair in $name::REGISTRATIONS.windows(2) {
                    assert!(pair[0].0 < pair[1].0, "registration numbers must ascend: {:?} then {:?}", pair[0], pair[1]);
                }
                assert_eq!($name::REGISTRATIONS.len(), $name::ALL.len());
            }

            #[test]
            fn labels_are_unique_and_non_empty() {
                for (position, registration) in $name::REGISTRATIONS.iter().enumerate() {
                    assert!(!registration.1.is_empty(), "registration {position} has no label");
                    let repeats = $name::REGISTRATIONS.iter().filter(|other| other.1 == registration.1).count();
                    assert_eq!(repeats, 1, "label {:?} is registered more than once", registration.1);
                }
            }

            #[test]
            fn every_member_round_trips_through_its_persisted_number() {
                for (member, registration) in $name::ALL.iter().zip($name::REGISTRATIONS) {
                    assert_eq!($name::try_from(member.code()), Ok(*member));
                    assert_eq!(member.code(), registration.0);
                    assert_eq!(member.label(), registration.1);
                }
            }

            #[test]
            fn a_number_the_registry_never_assigned_fails_to_decode() {
                let below = $name::ALL[0].code() - 1;
                let above = $name::ALL[$name::ALL.len() - 1].code() + 1;
                assert!($name::try_from(below).is_err(), "{below} decoded even though it was never assigned");
                assert!($name::try_from(above).is_err(), "{above} decoded even though it was never assigned");
            }

            #[test]
            fn the_registrations_a_release_froze_are_unchanged() {
                assert!(
                    $name::ALL.len() >= $name::FROZEN_PREFIX,
                    "the registry lost members a release had already written to disk"
                );
                let digest = frozen_prefix_digest($name::REGISTRATIONS, $name::FROZEN_PREFIX);
                assert_eq!(
                    digest, $name::FROZEN_DIGEST,
                    "the frozen registrations changed; blessing this requires frozen_digest: {digest:#018x}"
                );
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::{
        compact_code_domain::CompactCodeDomain,
        registry::{UnknownCompactCode, frozen_prefix_digest},
    };
    use pretty_assertions::{assert_eq, assert_ne};

    #[test]
    fn an_unknown_code_names_the_registry_that_refused_it() {
        let rejection = UnknownCompactCode::new(CompactCodeDomain::ValueKind, 42);
        assert_eq!(rejection.registry, CompactCodeDomain::ValueKind);
        assert_eq!(rejection.code, 42);
        assert_eq!(rejection.to_string(), "value-kind has no member with code 42");
    }

    #[test]
    fn appending_a_registration_leaves_the_frozen_prefix_digest_unchanged() {
        let released: &[(i16, &str)] = &[(1, "first"), (2, "second")];
        let appended: &[(i16, &str)] = &[(1, "first"), (2, "second"), (3, "third")];
        assert_eq!(frozen_prefix_digest(released, 2), frozen_prefix_digest(appended, 2));
    }

    #[test]
    fn altering_a_frozen_registration_changes_the_digest() {
        let released: &[(i16, &str)] = &[(1, "first"), (2, "second")];
        let renamed: &[(i16, &str)] = &[(1, "first"), (2, "renamed")];
        let renumbered: &[(i16, &str)] = &[(1, "first"), (3, "second")];
        let reordered: &[(i16, &str)] = &[(2, "second"), (1, "first")];
        assert_ne!(frozen_prefix_digest(released, 2), frozen_prefix_digest(renamed, 2));
        assert_ne!(frozen_prefix_digest(released, 2), frozen_prefix_digest(renumbered, 2));
        assert_ne!(frozen_prefix_digest(released, 2), frozen_prefix_digest(reordered, 2));
    }

    #[test]
    fn a_label_boundary_cannot_be_moved_without_changing_the_digest() {
        let split: &[(i16, &str)] = &[(1, "ab"), (2, "c")];
        let moved: &[(i16, &str)] = &[(1, "a"), (2, "bc")];
        assert_ne!(frozen_prefix_digest(split, 2), frozen_prefix_digest(moved, 2));
    }

    #[test]
    fn a_prefix_beyond_the_registry_digests_the_whole_registry() {
        let registrations: &[(i16, &str)] = &[(1, "first")];
        assert_eq!(frozen_prefix_digest(registrations, 9), frozen_prefix_digest(registrations, 1));
    }
}
