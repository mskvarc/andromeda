//! An entity selector as a query writes it, before its type expression is
//! expanded (clause 5.2.33).

use crate::{pattern::Pattern, type_selection::written_type_selection::WrittenTypeSelection};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::borrow::Cow;

/// One entity selector as a query writes it (clause 5.2.33): identifiers, an
/// identifier pattern and a type expression, the type optional since clause
/// 6.4.3.2 leaves it optional on a query.
///
/// A query's selection is the union of its selectors: `{id:A,type:T1}` and
/// `{id:B,type:T2}` select A of T1 and B of T2, never the cross product
/// `(A,T2)` that one flattened `id=A,B`/`type=T1,T2` pair would match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WrittenEntitySelector {
    /// The selector's entity identifiers (`id`).
    pub ids: Option<Vec<Identifier>>,
    /// A regular expression the selector's identifiers must match (`idPattern`).
    pub id_pattern: Option<Pattern>,
    /// The selector's parsed type-selection expression (clause 4.17 `type`).
    pub type_selection: Option<WrittenTypeSelection>,
}

impl WrittenEntitySelector {
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
        id: Option<&[Identifier]>,
        id_pattern: Option<&Pattern>,
        type_: Option<&WrittenTypeSelection>,
    ) -> Cow<'a, [Self]> {
        if structured.is_empty() {
            Cow::Owned(vec![Self {
                ids: id.map(<[Identifier]>::to_vec),
                id_pattern: id_pattern.cloned(), // clone: the implicit selector owns its values
                type_selection: type_.cloned(),  // clone: the implicit selector owns its values
            }])
        } else {
            Cow::Borrowed(structured)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{pattern::Pattern, type_selection::written_type_selection::WrittenTypeSelection, written_entity_selector::WrittenEntitySelector};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use std::{borrow::Cow, str::FromStr};

    fn identifier(text: &str) -> Identifier {
        Identifier::from_str(text).expect("valid identifier")
    }

    #[test]
    fn structured_selectors_are_returned_untouched() {
        let structured = vec![WrittenEntitySelector {
            ids: Some(vec![identifier("urn:ngsi-ld:A")]),
            ..WrittenEntitySelector::default()
        }];
        let flat = vec![identifier("urn:ngsi-ld:Z")];
        let effective = WrittenEntitySelector::effective(&structured, Some(&flat), None, None);
        assert!(matches!(effective, Cow::Borrowed(_)));
        pretty_assertions::assert_eq!(effective.as_ref(), structured.as_slice());
    }

    #[test]
    fn flat_fields_become_one_implicit_selector() {
        let ids = vec![identifier("urn:ngsi-ld:A"), identifier("urn:ngsi-ld:B")];
        let pattern = Pattern::from_str("^urn:ngsi-ld:.*").expect("valid pattern");
        let type_ = WrittenTypeSelection::from_str("T").expect("valid type");
        let effective = WrittenEntitySelector::effective(&[], Some(&ids), Some(&pattern), Some(&type_));
        pretty_assertions::assert_eq!(
            effective.as_ref(),
            [WrittenEntitySelector {
                ids: Some(ids),
                id_pattern: Some(pattern),
                type_selection: Some(type_),
            }]
        );
    }

    #[test]
    fn an_unconstrained_query_is_one_empty_selector() {
        let effective = WrittenEntitySelector::effective(&[], None, None, None);
        pretty_assertions::assert_eq!(effective.as_ref(), [WrittenEntitySelector::default()]);
    }
}
