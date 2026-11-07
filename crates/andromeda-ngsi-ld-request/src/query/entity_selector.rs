//! The per-selector form of a query's entity selection (clause 5.2.11).

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{pattern::Pattern, type_selection::grammar::TypeExprRaw};
use std::borrow::Cow;

/// One entity selector carried by a POST query body's `entities[]` array.
///
/// A GET query has no structured selectors — its flat `id`/`type`/`idPattern`
/// parameters describe one implicit selector — so this type exists to preserve
/// the per-selector grouping a POST body draws that a comma-joined parameter
/// cannot express: `{id:A,type:T1}` and `{id:B,type:T2}` are a union, not the
/// cross product `(A,T2)` that a flattened `id=A,B`/`type=T1,T2` would match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntitySelectorParams {
    /// The selector's entity identifiers (`id`).
    pub id: Option<Vec<Identifier>>,
    /// A regular expression the selector's identifiers must match (`idPattern`).
    pub id_pattern: Option<Pattern>,
    /// The selector's parsed type-selection expression (clause 4.17 `type`).
    pub type_: Option<TypeExprRaw>,
}

impl EntitySelectorParams {
    /// The selectors a query's entity selection is the union of (clause 5.2.33).
    ///
    /// A POST `entities[]` body supplies `structured` selectors, which are
    /// authoritative: the flat parameters are then only a summary of them. A GET
    /// request, or one whose selection arrived as protocol headers, has none, so
    /// its flat `id`/`idPattern`/`type` describe the one implicit selector. A
    /// query selecting nothing is one selector with no constraint.
    #[must_use]
    pub fn effective<'a>(structured: &'a [Self], id: Option<&[Identifier]>, id_pattern: Option<&Pattern>, type_: Option<&TypeExprRaw>) -> Cow<'a, [Self]> {
        if structured.is_empty() {
            Cow::Owned(vec![Self {
                id: id.map(<[Identifier]>::to_vec),
                id_pattern: id_pattern.cloned(), // clone: the implicit selector owns its values
                type_: type_.cloned(),           // clone: the implicit selector owns its values
            }])
        } else {
            Cow::Borrowed(structured)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::query::entity_selector::EntitySelectorParams;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::{pattern::Pattern, type_selection::grammar::TypeExprRaw};
    use std::{borrow::Cow, str::FromStr};

    fn identifier(text: &str) -> Identifier {
        Identifier::from_str(text).expect("valid identifier")
    }

    #[test]
    fn structured_selectors_are_returned_untouched() {
        let structured = vec![EntitySelectorParams {
            id: Some(vec![identifier("urn:ngsi-ld:A")]),
            ..EntitySelectorParams::default()
        }];
        let flat = vec![identifier("urn:ngsi-ld:Z")];
        let effective = EntitySelectorParams::effective(&structured, Some(&flat), None, None);
        assert!(matches!(effective, Cow::Borrowed(_)));
        pretty_assertions::assert_eq!(effective.as_ref(), structured.as_slice());
    }

    #[test]
    fn flat_fields_become_one_implicit_selector() {
        let ids = vec![identifier("urn:ngsi-ld:A"), identifier("urn:ngsi-ld:B")];
        let pattern = Pattern::from_str("^urn:ngsi-ld:.*").expect("valid pattern");
        let type_ = TypeExprRaw::from_str("T").expect("valid type");
        let effective = EntitySelectorParams::effective(&[], Some(&ids), Some(&pattern), Some(&type_));
        pretty_assertions::assert_eq!(
            effective.as_ref(),
            [EntitySelectorParams {
                id: Some(ids),
                id_pattern: Some(pattern),
                type_: Some(type_),
            }]
        );
    }

    #[test]
    fn an_unconstrained_query_is_one_empty_selector() {
        let effective = EntitySelectorParams::effective(&[], None, None, None);
        pretty_assertions::assert_eq!(effective.as_ref(), [EntitySelectorParams::default()]);
    }
}
