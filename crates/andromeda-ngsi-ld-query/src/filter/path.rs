//! Attribute paths addressed by a `q` predicate (clause 4.9).

use crate::error::QueryError;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use iri_rs::IriBuf;

/// An attribute path within a `q` expression, potentially including sub-attributes
/// and compound value navigation.
///
/// Example: `address[city].streetName` would have `attr = "address"`,
/// `sub_attrs = ["city"]`, `value_path = ["streetName"]`.
///
/// `attr` and `sub_attrs` are [`AttributeName`]s, which accept a compact short
/// name, a prefixed name or an expanded IRI. That is exactly the range this AST
/// spans: a freshly parsed filter carries whatever the client wrote, and an
/// expanded one carries IRIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrPath {
    /// The top-level attribute name (compact or expanded IRI).
    pub attr: AttributeName,
    /// Sub-attribute names for navigating nested attributes.
    pub sub_attrs: Vec<AttributeName>,
    /// Keys for navigating into compound values.
    ///
    /// Deliberately untyped: these are object keys inside an attribute's value,
    /// which the NGSI-LD data model does not constrain at all. They are
    /// genuinely free-form JSON keys, not names of anything the broker models.
    pub value_path: Vec<String>,
    /// Whether the path carried the `[*]` language-suppression marker, which
    /// matches a LanguageProperty against every language's value rather than
    /// against the one the `lang` parameter would otherwise pick (clause 4.9).
    pub match_all_languages: bool,
}

impl AttrPath {
    /// Build a path addressing an attribute's own value, with no sub-attribute
    /// and no compound value navigation.
    pub const fn simple(attr: AttributeName) -> AttrPath {
        AttrPath {
            attr,
            sub_attrs: Vec::new(),
            value_path: Vec::new(),
            match_all_languages: false,
        }
    }
}

/// Read an expanded IRI as the attribute name that addresses it.
///
/// Term expansion yields an absolute IRI, which the clause 5.2.4 name grammar
/// accepts, so the failure path only opens if a context produced something an
/// attribute cannot be keyed by.
pub fn expanded_attribute_name(iri: &IriBuf) -> Result<AttributeName, QueryError> {
    iri.as_str().parse().map_err(|source| QueryError::AttributeNameInvalid {
        value: iri.as_str().to_string(),
        source,
    })
}
