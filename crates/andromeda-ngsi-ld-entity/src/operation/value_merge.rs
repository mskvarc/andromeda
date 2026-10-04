//! Merging a written value into a stored one (clause 5.5.12), and what a
//! written value becomes where there is nothing to merge into.
//!
//! Clause 5.5.12 merges "up to an arbitrary depth, e.g. including going into
//! JSON objects representing a Property value": two structured nodes merge
//! member by member, a member set to the NGSI-LD Null is deleted, a `@json`
//! literal merges by IETF RFC 7396 and a language map by language. Anything
//! atomic (a value object, a node naming an identifier, a list) is replaced
//! wholesale, and so is a member holding more than one value. A deletion that
//! meets nothing has nothing to delete, so a written value merged into nothing
//! is the value with its deletions left out.

use crate::{
    expanded_value::{ExpandedValue, ValueNode},
    json_literal::JsonLiteral,
    language_entries::{LanguageEntries, LanguageEntry},
    operation::json_literal_merge::merge_json_literal,
    value_mode::{Fragment, FragmentMember, FragmentText},
};
use indexmap::IndexMap;
use serde_json::Value;
use std::mem;

/// A written value with every deletion it carries left out.
pub(crate) fn settled_value(value: &ExpandedValue<Fragment>) -> ExpandedValue {
    match value {
        // clone: the stored value owns its copy of the borrowed fragment.
        ExpandedValue::Literal(literal) => ExpandedValue::Literal(literal.clone()),
        ExpandedValue::Json(json) => ExpandedValue::Json(settled_json(json)),
        ExpandedValue::Node(node) => ExpandedValue::Node(settled_node(node)),
        ExpandedValue::List(items) => ExpandedValue::List(settled_values(items)),
    }
}

/// Written values, each with its deletions left out, in order.
pub(crate) fn settled_values(values: &[ExpandedValue<Fragment>]) -> Vec<ExpandedValue> {
    values.iter().map(settled_value).collect()
}

/// A written node with its deleted members left out.
fn settled_node(node: &ValueNode<Fragment>) -> ValueNode {
    let members = node
        .members()
        .iter()
        .filter_map(|(name, member)| match member {
            // clone: the stored node owns its member names.
            FragmentMember::Values(values) => Some((name.clone(), settled_values(values))),
            FragmentMember::Remove => None,
        })
        .collect();
    // clone: the stored node owns its identifier and types.
    ValueNode::new(node.id().cloned(), node.types().to_vec(), members)
}

/// A written JSON literal merged into nothing: by IETF RFC 7396 every key set
/// to the NGSI-LD Null string is left out, at any depth of nested objects.
pub(crate) fn settled_json(json: &JsonLiteral) -> JsonLiteral {
    let mut settled = Value::Null;
    merge_json_literal(&mut settled, json.get());
    JsonLiteral::new(settled)
}

/// A written language map with its deleted languages left out (clause
/// 4.5.18 admits several strings per language, all of which are kept).
pub(crate) fn settled_entries(entries: &LanguageEntries<Fragment>) -> LanguageEntries {
    LanguageEntries::new(
        entries
            .entries()
            .iter()
            .filter_map(|entry| match entry.text() {
                // clone: the stored map owns its copy of the borrowed fragment.
                FragmentText::Text(text) => Some(LanguageEntry::new(entry.language().cloned(), text.clone())),
                FragmentText::Remove => None,
            })
            .collect(),
    )
}

/// Merge one written value into one stored value (clause 5.5.12).
pub(crate) fn merge_value(base: &mut ExpandedValue, patch: &ExpandedValue<Fragment>) {
    match (base, patch) {
        (ExpandedValue::Json(base_json), ExpandedValue::Json(patch_json)) => merge_json(base_json, patch_json),
        // A node carrying `@id` names a resource and is as atomic as a value
        // object; only two anonymous nodes are structures to merge into.
        (ExpandedValue::Node(base_node), ExpandedValue::Node(patch_node)) if base_node.id().is_none() && patch_node.id().is_none() => {
            merge_node(base_node, patch_node);
        }
        (base, patch) => *base = settled_value(patch),
    }
}

/// Merge the values of one written member into the stored member: a single
/// value on both sides recurses, any other shape is replaced wholesale.
pub(crate) fn merge_values(base: &mut [ExpandedValue], patch: &[ExpandedValue<Fragment>]) -> MemberMerge {
    match (base, patch) {
        ([base_value], [patch_value]) => {
            merge_value(base_value, patch_value);
            MemberMerge::Merged
        }
        (_, _) => MemberMerge::Replace,
    }
}

/// Whether [`merge_values`] merged in place or left the member for the caller
/// to replace with the settled written values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MemberMerge {
    /// The single stored value took the single written one.
    Merged,
    /// The shapes differ: the written values replace the stored ones.
    Replace,
}

/// Merge a written `@json` literal into a stored one by IETF RFC 7396.
pub(crate) fn merge_json(base: &mut JsonLiteral, patch: &JsonLiteral) {
    let mut raw = mem::replace(base, JsonLiteral::new(Value::Null)).into_inner();
    merge_json_literal(&mut raw, patch.get());
    *base = JsonLiteral::new(raw);
}

/// Merge two anonymous structured nodes member by member.
fn merge_node(base: &mut ValueNode, patch: &ValueNode<Fragment>) {
    if !patch.types().is_empty() {
        // The node's `@type` is a JSON-LD keyword, which the merge copies
        // across verbatim rather than merging into.
        let (id, _, members) = mem::replace(base, ValueNode::new(None, Vec::new(), IndexMap::new())).into_parts();
        // clone: the stored node owns its copy of the borrowed types.
        *base = ValueNode::new(id, patch.types().to_vec(), members);
    }
    for (name, member) in patch.members() {
        match member {
            FragmentMember::Remove => {
                base.members_mut().shift_remove(name);
            }
            FragmentMember::Values(values) => match base.members_mut().get_mut(name) {
                Some(existing) => {
                    if merge_values(existing, values) == MemberMerge::Replace {
                        *existing = settled_values(values);
                    }
                }
                None => {
                    // clone: the stored node owns the member name it gains.
                    base.members_mut().insert(name.clone(), settled_values(values));
                }
            },
        }
    }
}

/// Merge a written language map into a stored one, language by language.
///
/// A language set to the NGSI-LD Null loses every entry; any other written
/// entry replaces the first entry of its language or is added.
pub(crate) fn merge_language_entries(base: &mut LanguageEntries, patch: &LanguageEntries<Fragment>) {
    for entry in patch.entries() {
        match entry.text() {
            FragmentText::Remove => base.entries_mut().retain(|held| held.language() != entry.language()),
            FragmentText::Text(text) => {
                // clone: the stored map owns its copy of the borrowed fragment.
                let written = LanguageEntry::new(entry.language().cloned(), text.clone());
                match base.entries_mut().iter_mut().find(|held| held.language() == entry.language()) {
                    Some(held) => *held = written,
                    None => base.entries_mut().push(written),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        expanded_value::{ExpandedValue, ValueNode},
        json_literal::JsonLiteral,
        language_entries::{LanguageEntries, LanguageEntry},
        node_id::NodeId,
        operation::{
            speed_entity::{member_name, number, text},
            value_merge::{merge_language_entries, merge_value, settled_json, settled_value},
        },
        value_mode::{Fragment, FragmentMember, FragmentText},
    };
    use indexmap::IndexMap;
    use langtag::LangTagBuf;
    use serde_json::json;

    fn tag(raw: &str) -> LangTagBuf {
        LangTagBuf::new(raw.to_owned()).expect("a language tag")
    }

    /// A JSON literal merged into nothing loses the keys set to the NGSI-LD
    /// Null string, at any depth of nested objects (IETF RFC 7396).
    #[test]
    fn a_settled_json_literal_leaves_its_deletions_out() {
        let written = JsonLiteral::new(json!({"a": 1, "b": "urn:ngsi-ld:null", "c": {"d": "urn:ngsi-ld:null", "e": null}}));
        pretty_assertions::assert_eq!(settled_json(&written), JsonLiteral::new(json!({"a": 1, "c": {"e": null}})));
    }

    /// A node naming an identifier is atomic: the written one replaces it.
    #[test]
    fn a_node_naming_an_identifier_is_replaced_rather_than_merged() {
        let held_members = [(member_name("https://example.org/a"), vec![number(1)])].into_iter().collect();
        let mut held: ExpandedValue = ExpandedValue::Node(ValueNode::new(Some(NodeId::from("urn:ngsi-ld:X:1")), Vec::new(), held_members));
        let written_members: IndexMap<_, _> = [(member_name("https://example.org/b"), FragmentMember::Values(vec![number(2)]))]
            .into_iter()
            .collect();
        let written: ExpandedValue<Fragment> = ExpandedValue::Node(ValueNode::new(Some(NodeId::from("urn:ngsi-ld:X:2")), Vec::new(), written_members));

        merge_value(&mut held, &written);

        pretty_assertions::assert_eq!(held, settled_value(&written));
    }

    /// A value object meeting a structured node is replaced wholesale.
    #[test]
    fn a_literal_is_replaced_by_the_written_value() {
        let mut held: ExpandedValue = text("old");
        merge_value(&mut held, &number(5));
        pretty_assertions::assert_eq!(held, number(5));
    }

    /// A written language entry replaces the first held entry of its
    /// language; a language set to the NGSI-LD Null loses every entry.
    #[test]
    fn a_language_merge_replaces_the_first_entry_and_deletes_by_language() {
        let mut held = LanguageEntries::new(vec![
            LanguageEntry::new(Some(tag("en")), "one".into()),
            LanguageEntry::new(Some(tag("en")), "two".into()),
            LanguageEntry::new(Some(tag("de")), "eins".into()),
        ]);
        let written = LanguageEntries::<Fragment>::new(vec![
            LanguageEntry::new(Some(tag("en")), FragmentText::Text("uno".into())),
            LanguageEntry::new(Some(tag("de")), FragmentText::Remove),
            LanguageEntry::new(None, FragmentText::Text("none".into())),
        ]);

        merge_language_entries(&mut held, &written);

        pretty_assertions::assert_eq!(
            held,
            LanguageEntries::new(vec![
                LanguageEntry::new(Some(tag("en")), "uno".into()),
                LanguageEntry::new(Some(tag("en")), "two".into()),
                LanguageEntry::new(None, "none".into()),
            ])
        );
    }
}
