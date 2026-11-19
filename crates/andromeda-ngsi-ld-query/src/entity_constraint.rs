//! One entity selector of a query (clause 5.2.33) as the query engine holds it,
//! generic over the stage its type names are in.

use crate::{
    identifier_axis::IdentifierAxis,
    name_stage::{Expanded, NameStage, Written},
    pattern::Pattern,
    term_expander::TermExpansionError,
    type_selection::TypeSelection,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::borrow::Cow;
use typed_builder::TypedBuilder;

/// What one entity selector of a query constrains: an identifier list, an
/// identifier pattern and a type selection a candidate must satisfy together
/// (clause 5.2.33).
///
/// A selector is the unit a POST query body's `entities[]` array is a union of.
/// `{id:A,type:T1}` and `{id:B,type:T2}` are two selectors, and an entity
/// matches the query when it satisfies *either* one, never the cross product
/// `(A,T2)` that neither named. A GET query, whose `id` and `type` parameters
/// are independent lists over one implicit group, is a single selector.
///
/// The selector is in the written stage while its type names are as the client
/// spelled them, and in the expanded stage once they are absolute IRIs.
#[derive(Debug, Clone, Default, PartialEq, Eq, TypedBuilder)]
#[builder(field_defaults(default = Default::default()))]
pub struct EntityConstraint<S: NameStage> {
    /// Resource identifiers this selector admits (clause 5.7.2 `id`). Empty
    /// means no identifier constraint.
    pub ids: Vec<Identifier>,
    /// Regular expression the entity id must match (clause 5.7.2 `idPattern`).
    pub id_pattern: Option<Pattern>,
    /// Type selection over entity types (clause 4.17 `type`), optional since
    /// clause 6.4.3.2 leaves `type` optional on a query.
    ///
    /// `None` when this selector places no type constraint.
    pub type_selection: Option<TypeSelection<S>>,
}

impl<S: NameStage> EntityConstraint<S> {
    /// The selectors a query's selection is the union of (clause 5.2.33).
    ///
    /// The `structured` selectors a query wrote as an array are authoritative,
    /// and its flat `id`, `idPattern` and `type` are then only a summary of
    /// them. A query that wrote none has the one selector its flat members
    /// describe, and a query selecting nothing is one selector with no
    /// constraint.
    #[must_use]
    pub fn effective<'a>(
        structured: &'a [Self],
        ids: Option<&[Identifier]>,
        id_pattern: Option<&Pattern>,
        type_selection: Option<&TypeSelection<S>>,
    ) -> Cow<'a, [Self]> {
        if structured.is_empty() {
            Cow::Owned(vec![Self {
                ids: ids.map_or_default(<[Identifier]>::to_vec),
                id_pattern: id_pattern.cloned(),         // clone: the implicit selector owns its values
                type_selection: type_selection.cloned(), // clone: the implicit selector owns its values
            }])
        } else {
            Cow::Borrowed(structured)
        }
    }

    /// The identifiers this selector names, or `None` when it names none and so
    /// places no identifier constraint.
    #[must_use]
    pub fn named_ids(&self) -> Option<&[Identifier]> {
        Some(self.ids.as_slice()).filter(|ids| !ids.is_empty())
    }

    /// What this selector constrains about the entity identifier, with the
    /// clause 5.2.33 precedence of `id` over `idPattern` applied.
    #[must_use]
    pub fn identifier_axis(&self) -> IdentifierAxis<'_> {
        IdentifierAxis::of(&self.ids, self.id_pattern.as_ref())
    }

    /// Whether this selector constrains a candidate at all.
    ///
    /// An unconstrained selector (no id, no pattern, no type) admits every
    /// entity, so a union that contains one is unconstrained on these axes.
    #[must_use]
    pub const fn constrains(&self) -> bool {
        !self.ids.is_empty() || self.id_pattern.is_some() || self.type_selection.is_some()
    }
}

impl EntityConstraint<Written> {
    /// The constraint in the expanded stage, for a stored selector whose type
    /// names are already absolute IRIs.
    ///
    /// # Errors
    ///
    /// Returns [`TermExpansionError::NotAnIri`] when a type name is no absolute
    /// IRI, which a selector expanded before it was stored never holds.
    pub fn into_expanded(self) -> Result<EntityConstraint<Expanded>, TermExpansionError> {
        Ok(EntityConstraint {
            ids: self.ids,
            id_pattern: self.id_pattern,
            type_selection: self.type_selection.map(TypeSelection::into_expanded).transpose()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_constraint::EntityConstraint,
        identifier_axis::IdentifierAxis,
        name_stage::Written,
        pattern::Pattern,
        term_expander::TermExpansionError,
        type_selection::TypeSelection,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use std::{borrow::Cow, str::FromStr};

    fn identifier(text: &str) -> Identifier {
        Identifier::from_str(text).expect("valid identifier")
    }

    #[test]
    fn structured_selectors_are_returned_untouched() {
        let structured = vec![EntityConstraint::<Written> {
            ids: vec![identifier("urn:ngsi-ld:A")],
            ..EntityConstraint::default()
        }];
        let flat = vec![identifier("urn:ngsi-ld:Z")];
        let effective = EntityConstraint::effective(&structured, Some(&flat), None, None);
        assert!(matches!(effective, Cow::Borrowed(_)));
        pretty_assertions::assert_eq!(effective.as_ref(), structured.as_slice());
    }

    #[test]
    fn flat_fields_become_one_implicit_selector() {
        let ids = vec![identifier("urn:ngsi-ld:A"), identifier("urn:ngsi-ld:B")];
        let pattern = Pattern::from_str("^urn:ngsi-ld:.*").expect("valid pattern");
        let type_ = TypeSelection::<Written>::from_str("T").expect("valid type");
        let effective = EntityConstraint::effective(&[], Some(&ids), Some(&pattern), Some(&type_));
        pretty_assertions::assert_eq!(
            effective.as_ref(),
            [EntityConstraint {
                ids,
                id_pattern: Some(pattern),
                type_selection: Some(type_),
            }]
        );
    }

    #[test]
    fn an_unconstrained_query_is_one_empty_selector() {
        let effective = EntityConstraint::<Written>::effective(&[], None, None, None);
        pretty_assertions::assert_eq!(effective.as_ref(), [EntityConstraint::default()]);
        assert!(!effective[0].constrains());
    }

    #[test]
    fn a_selector_naming_no_identifier_has_no_named_ids() {
        let none = EntityConstraint::<Written>::default();
        assert!(none.named_ids().is_none());
        let one = EntityConstraint::<Written> {
            ids: vec![identifier("urn:ngsi-ld:A:1")],
            ..EntityConstraint::default()
        };
        pretty_assertions::assert_eq!(one.named_ids(), Some([identifier("urn:ngsi-ld:A:1")].as_slice()));
    }

    #[test]
    fn a_stored_constraint_over_absolute_iris_is_in_the_expanded_stage_and_one_over_a_bare_term_is_refused() {
        let stored = EntityConstraint::<Written> {
            ids: vec![identifier("urn:ngsi-ld:A:1")],
            type_selection: Some("https://example.com/Vehicle".parse().expect("a valid type")),
            ..EntityConstraint::default()
        };
        let expanded = stored.into_expanded().expect("an absolute IRI");
        pretty_assertions::assert_eq!(expanded.ids, vec![identifier("urn:ngsi-ld:A:1")]);
        assert!(expanded.type_selection.is_some());

        let bare = EntityConstraint::<Written> {
            type_selection: Some("Vehicle".parse().expect("a valid type")),
            ..EntityConstraint::default()
        };
        pretty_assertions::assert_eq!(bare.into_expanded(), Err(TermExpansionError::NotAnIri));
    }

    /// Clause 5.2.33: a selector naming an `id` reads as constraining by id
    /// alone, whatever `idPattern` it also carries.
    #[test]
    fn a_selector_with_an_id_and_an_id_pattern_constrains_by_id() {
        let pattern = "^urn:nomatch".parse::<Pattern>().expect("valid pattern");
        let selector = EntityConstraint::<Written>::builder()
            .ids(vec![identifier("urn:ngsi-ld:A:1")])
            .id_pattern(Some(pattern))
            .build();
        assert!(matches!(selector.identifier_axis(), IdentifierAxis::Ids(_)));
    }
}
