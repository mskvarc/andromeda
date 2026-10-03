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
//! "equivalent to the one passed as a parameter" requires.

use andromeda_ngsi_ld_lexicon::identifier::{Identifier, IdentifierParseError};
use serde_json::{Map, Value};
use thiserror::Error;

/// The JSON-LD keyword spellings a compact Fragment may name its Entity with.
const ID_MEMBERS: [&str; 2] = ["id", "@id"];

/// Why a Fragment's `id` does not name the target the request path names.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FragmentEntityIdError {
    /// The `id` is not a string.
    #[error("the Fragment 'id' must be a string")]
    NotAString,
    /// The `id` is not an identifier.
    #[error("the Fragment 'id' is not an identifier: {source}")]
    Unparsable {
        /// Why it is not an identifier.
        #[source]
        source: IdentifierParseError,
    },
    /// The `id` names another Entity than the request targets.
    #[error("the Fragment 'id' {written} is not the target entity {target}")]
    OtherEntity {
        /// The `id` the Fragment carries.
        written: Box<Identifier>,
        /// The Entity the request targets.
        target: Box<Identifier>,
    },
}

/// Check that every `id` the Fragment `members` carry, in either keyword
/// spelling, names `target` (clause 5.4). A Fragment naming none is admitted.
///
/// # Errors
///
/// Returns the [`FragmentEntityIdError`] for the first `id` that is not a
/// string, not an identifier, or not `target`.
pub fn admit_fragment_entity_id(members: &Map<String, Value>, target: &Identifier) -> Result<(), FragmentEntityIdError> {
    ID_MEMBERS
        .iter()
        .filter_map(|member| members.get(*member))
        .try_for_each(|written| admit_written_id(written, target))
}

/// Check one written `id` against `target`.
fn admit_written_id(written: &Value, target: &Identifier) -> Result<(), FragmentEntityIdError> {
    let raw = written.as_str().ok_or(FragmentEntityIdError::NotAString)?;
    let written = raw.parse::<Identifier>().map_err(|source| FragmentEntityIdError::Unparsable { source })?;
    if written == *target {
        return Ok(());
    }
    Err(FragmentEntityIdError::OtherEntity {
        written: Box::new(written),
        // clone: the error owns the target it names
        target: Box::new(target.clone()),
    })
}

#[cfg(test)]
mod tests {
    use crate::fragment_entity_id::{FragmentEntityIdError, admit_fragment_entity_id};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn target() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("valid identifier")
    }

    fn identifier(raw: &str) -> Identifier {
        raw.parse().expect("valid identifier")
    }

    fn admitted(body: &Value) -> Result<(), FragmentEntityIdError> {
        admit_fragment_entity_id(body.as_object().expect("a fragment is an object"), &target())
    }

    /// The refusal naming `written` as the other Entity.
    fn other_entity(written: &str) -> FragmentEntityIdError {
        FragmentEntityIdError::OtherEntity {
            written: Box::new(identifier(written)),
            target: Box::new(target()),
        }
    }

    /// Clause 5.4: the `id` is "optional for certain bindings where it can be
    /// determined from the operation signature".
    #[test]
    fn a_fragment_naming_no_id_is_admitted() {
        assert_eq!(admitted(&json!({"speed": [{"type": "Property", "value": 1}]})), Ok(()));
    }

    /// Clause 5.4: the `id` "shall be equal to the id of the target", in
    /// either spelling of the JSON-LD keyword.
    #[test]
    fn a_fragment_naming_its_target_is_admitted() {
        assert_eq!(admitted(&json!({"id": "urn:ngsi-ld:Vehicle:1"})), Ok(()));
        assert_eq!(admitted(&json!({"@id": "urn:ngsi-ld:Vehicle:1"})), Ok(()));
    }

    /// Clause 5.6.12.4 targets the evolution whose id "is equivalent to the
    /// one passed as a parameter"; RFC 8141 compares the scheme and the
    /// namespace identifier without regard to case.
    #[test]
    fn an_equivalent_spelling_of_the_target_is_admitted() {
        assert_eq!(admitted(&json!({"id": "URN:NGSI-LD:Vehicle:1"})), Ok(()));
    }

    /// Clause 5.4: an `id` naming another Entity is not the target's.
    #[test]
    fn an_id_naming_another_entity_is_refused() {
        assert_eq!(admitted(&json!({"id": "urn:ngsi-ld:Vehicle:2"})), Err(other_entity("urn:ngsi-ld:Vehicle:2")));
        assert_eq!(admitted(&json!({"@id": "urn:ngsi-ld:Vehicle:2"})), Err(other_entity("urn:ngsi-ld:Vehicle:2")));
    }

    /// RFC 8141 keeps the namespace-specific string case-sensitive, so a
    /// differently cased one names another Entity.
    #[test]
    fn a_case_differing_namespace_specific_string_is_another_entity() {
        assert_eq!(admitted(&json!({"id": "urn:ngsi-ld:vehicle:1"})), Err(other_entity("urn:ngsi-ld:vehicle:1")));
    }

    /// Both spellings of the keyword name the target: one naming it cannot
    /// carry the other past the check.
    #[test]
    fn both_spellings_are_held_to_the_target() {
        assert_eq!(
            admitted(&json!({"id": "urn:ngsi-ld:Vehicle:1", "@id": "urn:ngsi-ld:Vehicle:2"})),
            Err(other_entity("urn:ngsi-ld:Vehicle:2"))
        );
    }

    /// Clause 5.5.4: an `id` that is no string is no identifier.
    #[test]
    fn an_id_that_is_not_a_string_is_refused() {
        assert_eq!(admitted(&json!({"id": 7})), Err(FragmentEntityIdError::NotAString));
    }

    /// Clause 5.5.4: an `id` that does not parse as a URI is no identifier.
    #[test]
    fn an_id_that_is_not_an_identifier_is_refused() {
        assert!(matches!(
            admitted(&json!({"id": "not an identifier"})),
            Err(FragmentEntityIdError::Unparsable { .. })
        ));
    }
}
