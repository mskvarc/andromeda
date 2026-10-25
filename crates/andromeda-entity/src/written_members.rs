//! The members a written Entity Fragment names, as the attribute names Update
//! Attributes and Append Attributes report.
//!
//! Clauses 5.6.2.5 and 5.6.3.5 report "the list of Attributes" a request
//! updated or appended, and a fragment may name `scope` (clause 4.18) and the
//! entity `expiresAt` (clause 4.22) beside its attributes, each reported by
//! its expanded name like an attribute. The lists are ordered by expanded
//! name.

use crate::{entity_invariant::EntityInvariant, error::EntityError};
use andromeda_ngsi_ld_entity::{document::EntityDocument, reads::member_name::MemberName};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// Every member `fragment` writes, by expanded name.
///
/// # Errors
///
/// Reports [`EntityError::Internal`] when the expanded name of `scope` or of
/// the entity `expiresAt` does not read as an attribute name.
pub(crate) fn written_member_names(fragment: &EntityDocument) -> Result<Vec<AttributeName>, EntityError> {
    let scope = fragment.scope().map(|_| MemberName::Scope);
    let expires_at = fragment.expires_at().map(|_| MemberName::ExpiresAt);
    member_attribute_names(
        scope
            .into_iter()
            .chain(expires_at)
            .chain(fragment.attributes().keys().map(MemberName::Attribute)),
    )
}

/// The attribute names of `members`, ordered by expanded name.
///
/// # Errors
///
/// As [`written_member_names`].
pub(crate) fn member_attribute_names<'a>(members: impl IntoIterator<Item = MemberName<'a>>) -> Result<Vec<AttributeName>, EntityError> {
    let mut names = members
        .into_iter()
        .map(|member| match member {
            MemberName::Attribute(name) => Ok(name.clone()), // clone: the report outlives the fragment it names
            MemberName::Scope | MemberName::ExpiresAt => member
                .as_str()
                .parse()
                .map_err(|source| EntityError::Internal(EntityInvariant::ReservedMemberName(source))),
        })
        .collect::<Result<Vec<AttributeName>, EntityError>>()?;
    names.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
    Ok(names)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::written_members::written_member_names;
    use andromeda_ngsi_ld_entity::document::EntityDocument;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use serde_json::json;

    /// Clause 5.6.2.5: the report names the scope and the entity expiry a
    /// fragment writes beside its attributes, by expanded name, in order.
    #[test]
    fn a_fragment_reports_its_scope_and_expiry_beside_its_attributes() {
        let fragment = json!([{
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/a"}],
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2030-01-01T00:00:00Z"}],
            "https://example.org/z": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}],
            "https://example.org/a": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 2}]}]
        }]);
        let document = EntityDocument::from_slice(&serde_json::to_vec(&fragment).expect("JSON")).expect("a written fragment");
        let names = written_member_names(&document).expect("the core members are attribute names");
        pretty_assertions::assert_eq!(
            names.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec![
                "https://example.org/a",
                "https://example.org/z",
                "https://uri.etsi.org/ngsi-ld/expiresAt",
                "https://uri.etsi.org/ngsi-ld/scope"
            ]
        );
    }
}
