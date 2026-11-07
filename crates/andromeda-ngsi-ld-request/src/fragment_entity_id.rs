//! The `id` an NGSI-LD Fragment carries, held to the target a request path
//! names.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.4 lets a Fragment carry an `id`, "optional
//! for certain bindings where it can be determined from the operation
//! signature", and requires that "It shall be equal to the id of the target
//! (mutated) NGSI-LD element". Where a request path names the target, an
//! absent `id` is therefore admitted and a present one must name that same
//! Entity. A Fragment naming another is not the Fragment the operation takes,
//! which clause 5.5.4 answers with `BadRequestData`.
//!
//! Equality is the [`Identifier`]'s own: the URI is normalised as it is parsed
//! (RFC 8141 equivalence for a URN, RFC 3986 normalisation for a URL or an
//! IRI), so an equivalent spelling names the same Entity, as clause 5.6.12.4's
//! "equivalent to the one passed as a parameter" requires. A Fragment whose
//! `id` is no identifier is refused when it is decoded.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use thiserror::Error;

/// The `id` of a Fragment names another Entity than the request targets.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("the Fragment 'id' {written} is not the target entity {target}")]
pub struct FragmentEntityIdError {
    /// The `id` the Fragment carries.
    pub written: Box<Identifier>,
    /// The Entity the request targets.
    pub target: Box<Identifier>,
}

/// Check that the `id` a Fragment carries, when it carries one, names `target`
/// (clause 5.4). A Fragment naming none is admitted.
///
/// # Errors
///
/// Returns the [`FragmentEntityIdError`] for an `id` that is not `target`.
pub fn admit_fragment_entity_id(written: Option<&Identifier>, target: &Identifier) -> Result<(), FragmentEntityIdError> {
    match written {
        Some(written) if written != target => Err(FragmentEntityIdError {
            // clone: the error owns the identifiers it names
            written: Box::new(written.clone()),
            target: Box::new(target.clone()),
        }),
        Some(_) | None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::fragment_entity_id::{FragmentEntityIdError, admit_fragment_entity_id};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;

    fn target() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("valid identifier")
    }

    fn identifier(raw: &str) -> Identifier {
        raw.parse().expect("valid identifier")
    }

    fn admitted(written: Option<&str>) -> Result<(), FragmentEntityIdError> {
        admit_fragment_entity_id(written.map(identifier).as_ref(), &target())
    }

    /// The refusal naming `written` as the other Entity.
    fn other_entity(written: &str) -> FragmentEntityIdError {
        FragmentEntityIdError {
            written: Box::new(identifier(written)),
            target: Box::new(target()),
        }
    }

    /// Clause 5.4: the `id` is "optional for certain bindings where it can be
    /// determined from the operation signature".
    #[test]
    fn a_fragment_naming_no_id_is_admitted() {
        assert_eq!(admitted(None), Ok(()));
    }

    /// Clause 5.4: the `id` "shall be equal to the id of the target".
    #[test]
    fn a_fragment_naming_its_target_is_admitted() {
        assert_eq!(admitted(Some("urn:ngsi-ld:Vehicle:1")), Ok(()));
    }

    /// Clause 5.6.12.4 targets the evolution whose id "is equivalent to the
    /// one passed as a parameter"; RFC 8141 compares the scheme and the
    /// namespace identifier without regard to case.
    #[test]
    fn an_equivalent_spelling_of_the_target_is_admitted() {
        assert_eq!(admitted(Some("URN:NGSI-LD:Vehicle:1")), Ok(()));
    }

    /// Clause 5.4: an `id` naming another Entity is not the target's.
    #[test]
    fn an_id_naming_another_entity_is_refused() {
        assert_eq!(admitted(Some("urn:ngsi-ld:Vehicle:2")), Err(other_entity("urn:ngsi-ld:Vehicle:2")));
    }

    /// RFC 8141 keeps the namespace-specific string case-sensitive, so a
    /// differently cased one names another Entity.
    #[test]
    fn a_case_differing_namespace_specific_string_is_another_entity() {
        assert_eq!(admitted(Some("urn:ngsi-ld:vehicle:1")), Err(other_entity("urn:ngsi-ld:vehicle:1")));
    }
}
