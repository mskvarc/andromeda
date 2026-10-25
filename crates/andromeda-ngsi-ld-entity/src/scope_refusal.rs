//! Why the entity `scope` member is not a value clause 4.18 admits.

use crate::{member_shape::MemberShape, refusal_wording::compact_term};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, scope::path::ScopePathError};
use thiserror::Error;

/// Why the `scope` member is not a value clause 4.18 admits.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ScopeRefusal {
    /// One node of the member carries no string value.
    #[error("a scope value is not a string")]
    NotAString,
    /// One value is not a scope path.
    #[error("a scope value is not a scope path: {source}")]
    NotAScopePath {
        /// Why it is not a scope path.
        source: ScopePathError,
    },
    /// The NGSI-LD Null appears beside a scope path; clause 4.18 admits it only
    /// "in case of deleted scopes".
    #[error("the NGSI-LD Null names a deleted scope, so it cannot appear beside a scope path")]
    NullBesidePath,
    /// The NGSI-LD Null appears more than once; clause 4.18: "There shall at
    /// most be one instance of the scope property per Entity".
    #[error("the NGSI-LD Null appears more than once in the scope member, which states at most one deleted scope")]
    RepeatedNull,
    /// The member lists no path at all.
    #[error("the scope member lists no scope path")]
    Empty,
    /// A reified Scope instance carries its value under a member other than a
    /// Property's `value`; clause 4.5.6 makes it "a temporal representation of
    /// a Property".
    #[error("a reified scope is a Property, so it carries its scope paths under `value`")]
    NotAProperty,
    /// A reified Scope instance carries a member clause 4.5.6 does not give
    /// it: its only sub-Properties are the non-reified Temporal Properties.
    #[error("a reified scope cannot carry '{member}': its only sub-Properties are createdAt, modifiedAt, deletedAt and observedAt")]
    MemberForbidden {
        /// The member it carries.
        member: MemberShape,
    },
    /// A reified Scope instance carries a sub-attribute; clause 4.5.6 gives it
    /// only the non-reified Temporal Properties as sub-Properties.
    #[error("a reified scope cannot carry the sub-attribute '{}': its only sub-Properties are createdAt, modifiedAt, deletedAt and observedAt", compact_term(.attribute))]
    SubAttributeForbidden {
        /// The sub-attribute it carries.
        attribute: AttributeName,
    },
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{member_shape::MemberShape, scope_refusal::ScopeRefusal};

    #[test]
    fn a_forbidden_member_is_named_by_its_term() {
        pretty_assertions::assert_eq!(
            ScopeRefusal::MemberForbidden {
                member: MemberShape::DatasetId
            }
            .to_string(),
            "a reified scope cannot carry 'datasetId': its only sub-Properties are createdAt, modifiedAt, deletedAt and observedAt"
        );
    }

    #[test]
    fn a_forbidden_sub_attribute_is_named_by_its_compact_term_where_the_core_context_has_one() {
        let refusal = ScopeRefusal::SubAttributeForbidden {
            attribute: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        };
        pretty_assertions::assert_eq!(
            refusal.to_string(),
            "a reified scope cannot carry the sub-attribute 'location': its only sub-Properties are createdAt, modifiedAt, deletedAt and observedAt"
        );
    }
}
