//! Turning written values into state values.
//!
//! A written value may hold the NGSI-LD Null inside a structured value or a
//! language map (clause 5.5.12). State cannot: the conversion yields `None`
//! when a deletion stands anywhere inside.

use crate::{
    expanded_value::{ExpandedValue, ValueNode},
    language_entries::{LanguageEntries, LanguageEntry},
    value_mode::{Fragment, FragmentMember, FragmentText},
};

impl ExpandedValue<Fragment> {
    /// The value as state, or `None` when a deletion stands inside it.
    #[must_use]
    pub fn into_stored(self) -> Option<ExpandedValue> {
        match self {
            ExpandedValue::Literal(literal) => Some(ExpandedValue::Literal(literal)),
            ExpandedValue::Json(json) => Some(ExpandedValue::Json(json)),
            ExpandedValue::List(items) => into_stored_values(items).map(ExpandedValue::List),
            ExpandedValue::Node(node) => {
                let (id, types, members) = node.into_parts();
                let members = members
                    .into_iter()
                    .map(|(name, member)| match member {
                        FragmentMember::Values(values) => into_stored_values(values).map(|values| (name, values)),
                        FragmentMember::Remove => None,
                    })
                    .collect::<Option<_>>()?;
                Some(ExpandedValue::Node(ValueNode::new(id, types, members)))
            }
        }
    }
}

/// Every value as state, or `None` when a deletion stands inside one.
pub(crate) fn into_stored_values(values: Vec<ExpandedValue<Fragment>>) -> Option<Vec<ExpandedValue>> {
    values.into_iter().map(ExpandedValue::into_stored).collect()
}

impl LanguageEntries<Fragment> {
    /// The entries as state, or `None` when one deletes a language.
    #[must_use]
    pub fn into_stored(self) -> Option<LanguageEntries> {
        self.into_entries()
            .into_iter()
            .map(|entry| {
                let (language, text) = entry.into_parts();
                match text {
                    FragmentText::Text(text) => Some(LanguageEntry::new(language, text)),
                    FragmentText::Remove => None,
                }
            })
            .collect::<Option<Vec<_>>>()
            .map(LanguageEntries::new)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        expanded_value::{ExpandedValue, ValueNode},
        json_literal::JsonLiteral,
        value_mode::{Fragment, FragmentMember},
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use indexmap::IndexMap;
    use serde_json::json;

    fn name(raw: &str) -> AttributeName {
        raw.parse().expect("a valid name")
    }

    #[test]
    fn a_value_without_a_deletion_becomes_state() {
        let mut members = IndexMap::new();
        members.insert(
            name("https://ex.org/a"),
            FragmentMember::Values(vec![ExpandedValue::Json(JsonLiteral::new(json!(1)))]),
        );
        let value: ExpandedValue<Fragment> = ExpandedValue::Node(ValueNode::new(None, Vec::new(), members));
        assert!(value.into_stored().is_some());
    }

    #[test]
    fn a_member_deletion_cannot_become_state() {
        let mut members = IndexMap::new();
        members.insert(name("https://ex.org/a"), FragmentMember::Remove);
        let value: ExpandedValue<Fragment> = ExpandedValue::Node(ValueNode::new(None, Vec::new(), members));
        assert!(value.into_stored().is_none());
    }
}
