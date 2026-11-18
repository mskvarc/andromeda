//! One entity selector of a query (clause 5.2.33), its type selection expanded.

use crate::{identifier_axis::IdentifierAxis, pattern::Pattern, type_selection::expanded_type_selection::ExpandedTypeSelection};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use typed_builder::TypedBuilder;

/// One entity selector: an identifier list, an identifier pattern and a type
/// selection a candidate must satisfy together (clause 5.2.33).
///
/// A selector is the unit a POST query body's `entities[]` array is a union of.
/// `{id:A,type:T1}` and `{id:B,type:T2}` are two selectors, and an entity
/// matches the query when it satisfies *either* one, never the cross product
/// `(A,T2)` that neither named. A GET query, whose `id` and `type` parameters
/// are independent lists over one implicit group, is a single selector.
#[derive(Debug, Clone, Default, TypedBuilder)]
#[builder(field_defaults(default = Default::default()))]
pub struct ExpandedEntitySelector {
    /// Resource identifiers this selector admits. Empty means no identifier
    /// constraint (clause 5.7.2 `id`).
    pub ids: Vec<Identifier>,
    /// Regular expression the entity id must match (clause 5.7.2 `idPattern`).
    pub id_pattern: Option<Pattern>,
    /// Type selection over entity type IRIs (clause 4.17 `type`).
    ///
    /// `None` when this selector places no type constraint.
    pub type_selection: Option<ExpandedTypeSelection>,
}

impl ExpandedEntitySelector {
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

#[cfg(test)]
mod tests {
    use crate::{expanded_entity_selector::ExpandedEntitySelector, identifier_axis::IdentifierAxis, pattern::Pattern};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;

    /// Clause 5.2.33: a selector naming an `id` reads as constraining by id
    /// alone, whatever `idPattern` it also carries.
    #[test]
    fn a_selector_with_an_id_and_an_id_pattern_constrains_by_id() {
        let pattern = "^urn:nomatch".parse::<Pattern>().expect("valid pattern");
        let id: Identifier = "urn:ngsi-ld:A:1".parse().expect("valid id");
        let selector = ExpandedEntitySelector::builder().ids(vec![id]).id_pattern(Some(pattern)).build();
        assert!(matches!(selector.identifier_axis(), IdentifierAxis::Ids(_)));
    }
}
