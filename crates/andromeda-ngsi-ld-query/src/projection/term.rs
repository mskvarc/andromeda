//! Terms of the `attrs`, `pick` and `omit` selection lists (clause 6.3.15).

use iri_rs::IriBuf;

/// One member of an attribute selection list.
///
/// Clause 6.3.15 lets `pick` and `omit` name three structural members of the
/// representation alongside attributes. Those three are JSON keys of the
/// NGSI-LD representation itself, not attributes, so they have no IRI to expand
/// to and the renderer treats each of them differently from an attribute. The
/// enum is what stops that distinction from being re-derived by comparing the
/// term against three string literals wherever it matters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionTerm {
    /// The entity identifier.
    Id,
    /// The entity type.
    Type,
    /// The entity scope.
    Scope,
    /// One attribute, named by its expanded IRI, together with the
    /// clause 4.21 linked-entity sub-projection carried inside `{...}` — empty
    /// when this term selects no linked entity.
    Attribute {
        /// The expanded IRI naming this attribute.
        iri: IriBuf,
        /// The sub-projection to apply to the entity this attribute (a
        /// relationship) links to.
        linked: Vec<ProjectionTerm>,
    },
}

impl ProjectionTerm {
    /// The `id` keyword of clause 6.3.15.
    pub const ID_KEYWORD: &'static str = "id";
    /// The `type` keyword of clause 6.3.15.
    pub const TYPE_KEYWORD: &'static str = "type";
    /// The `scope` keyword of clause 6.3.15.
    pub const SCOPE_KEYWORD: &'static str = "scope";

    /// Whether this term names the attribute an entity keys as `key` and whose
    /// key expands to `expanded`.
    ///
    /// Both forms are tested because an entity's attribute map may be keyed by
    /// a compact term or by an IRI, depending on which side of expansion the
    /// representation sits on. A keyword term never names an attribute.
    pub fn matches_attribute(&self, key: &str, expanded: &IriBuf) -> bool {
        match self {
            ProjectionTerm::Attribute { iri, .. } => iri == expanded || iri.as_str() == key,
            ProjectionTerm::Id | ProjectionTerm::Type | ProjectionTerm::Scope => false,
        }
    }

    /// The clause 4.21 linked-entity sub-projection carried by this term.
    ///
    /// Empty for the clause 6.3.15 keywords and for an attribute term that
    /// selects no linked entity.
    pub fn linked_terms(&self) -> &[ProjectionTerm] {
        match self {
            ProjectionTerm::Attribute { linked, .. } => linked,
            ProjectionTerm::Id | ProjectionTerm::Type | ProjectionTerm::Scope => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    #[test]
    fn a_keyword_term_never_names_an_attribute() {
        // Clause 6.3.15: these select structural members of the
        // representation, so they cannot select an attribute of the same name.
        for term in [ProjectionTerm::Id, ProjectionTerm::Type, ProjectionTerm::Scope] {
            assert!(!term.matches_attribute("id", &iri("https://example.com/id")), "{term:?}");
        }
    }

    #[test]
    fn an_attribute_term_matches_either_the_compact_key_or_its_expansion() {
        let term = ProjectionTerm::Attribute {
            iri: iri("https://example.com/speed"),
            linked: Vec::new(),
        };
        assert!(term.matches_attribute("speed", &iri("https://example.com/speed")));
        assert!(term.matches_attribute("https://example.com/speed", &iri("urn:unrelated")));
        assert!(!term.matches_attribute("colour", &iri("https://example.com/colour")));
    }

    #[test]
    fn a_keyword_term_carries_no_linked_terms() {
        for term in [ProjectionTerm::Id, ProjectionTerm::Type, ProjectionTerm::Scope] {
            assert!(term.linked_terms().is_empty(), "{term:?}");
        }
    }

    #[test]
    fn a_leaf_attribute_term_carries_no_linked_terms() {
        let term = ProjectionTerm::Attribute {
            iri: iri("https://example.com/speed"),
            linked: Vec::new(),
        };
        assert!(term.linked_terms().is_empty());
    }

    #[test]
    fn an_attribute_term_exposes_its_linked_sub_projection() {
        let child = ProjectionTerm::Attribute {
            iri: iri("https://example.com/temperature"),
            linked: Vec::new(),
        };
        let term = ProjectionTerm::Attribute {
            iri: iri("https://example.com/observation"),
            linked: vec![child.clone()],
        };
        assert_eq!(term.linked_terms(), &[child]);
    }
}
