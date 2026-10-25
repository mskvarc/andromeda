//! State values read as written values.
//!
//! A state value holds no NGSI-LD Null, so as a written value it is one whose
//! every structured member and language entry is set (clause 5.5.12 gives the
//! deletion its place only in a written fragment).

use crate::{
    expanded_value::{ExpandedValue, ValueNode},
    language_entries::{LanguageEntries, LanguageEntry},
    value_mode::{Fragment, FragmentMember, FragmentText},
};

impl ExpandedValue {
    /// The value as a written value, every member set.
    #[must_use]
    pub fn to_fragment(&self) -> ExpandedValue<Fragment> {
        match self {
            ExpandedValue::Literal(literal) => ExpandedValue::Literal(literal.clone()), // clone: the written value owns its literal
            ExpandedValue::Json(json) => ExpandedValue::Json(json.clone()),             // clone: the written value owns its literal
            ExpandedValue::List(items) => ExpandedValue::List(items.iter().map(ExpandedValue::to_fragment).collect()),
            ExpandedValue::Node(node) => ExpandedValue::Node(ValueNode::new(
                node.id().cloned(),    // clone: the written node owns its identifier
                node.types().to_vec(), // clone: the written node owns its types
                node.members()
                    .iter()
                    .map(|(name, values)| {
                        // clone: the written node owns its member names
                        (name.clone(), FragmentMember::Values(values.iter().map(ExpandedValue::to_fragment).collect()))
                    })
                    .collect(),
            )),
        }
    }
}

impl LanguageEntries {
    /// The entries as written entries, every language set.
    #[must_use]
    pub fn to_fragment(&self) -> LanguageEntries<Fragment> {
        LanguageEntries::new(
            self.entries()
                .iter()
                .map(|entry| LanguageEntry::new(entry.language().cloned(), FragmentText::Text(entry.text().clone()))) // clone: the written entry owns its text
                .collect(),
        )
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        expanded_value::{ExpandedValue, ValueNode},
        expanded_value_name::ExpandedValueName,
        language_entries::{LanguageEntries, LanguageEntry},
        literal::{Literal, LiteralValue},
        value_mode::{Fragment, FragmentMember, FragmentText},
    };
    use andromeda_json_literal::JsonLiteral;

    use indexmap::IndexMap;
    use serde_json::json;

    fn name(raw: &str) -> ExpandedValueName {
        raw.parse().expect("a valid name")
    }

    #[test]
    fn a_structured_value_reads_as_a_fragment_with_every_member_set_and_back() {
        let mut members = IndexMap::new();
        members.insert(
            name("https://ex.org/a"),
            vec![ExpandedValue::Json(JsonLiteral::admit(&json!(1).to_string()).expect("a JSON literal"))],
        );
        members.insert(
            name("https://ex.org/b"),
            vec![ExpandedValue::List(vec![ExpandedValue::Literal(Literal::plain(LiteralValue::Boolean(true)))])],
        );
        let stored: ExpandedValue = ExpandedValue::Node(ValueNode::new(None, Vec::new(), members));

        let written = stored.to_fragment();

        let ExpandedValue::Node(node) = &written else {
            panic!("a node stays a node");
        };
        assert!(node.members().values().all(|member| matches!(member, FragmentMember::Values(_))));
        pretty_assertions::assert_eq!(written.into_stored(), Some(stored));
    }

    #[test]
    fn language_entries_read_as_set_texts_and_back() {
        let stored = LanguageEntries::new(vec![LanguageEntry::new(None, "Station".into())]);

        let written: LanguageEntries<Fragment> = stored.to_fragment();

        assert!(matches!(written.entries(), [entry] if *entry.text() == FragmentText::Text("Station".into())));
        pretty_assertions::assert_eq!(written.into_stored(), Some(stored));
    }
}
