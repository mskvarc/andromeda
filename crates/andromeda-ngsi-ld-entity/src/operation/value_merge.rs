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
    language_entries::{LanguageEntries, LanguageEntry},
    value_mode::{Fragment, FragmentMember, FragmentText},
};
use andromeda_json_literal::admission::JsonLiteralError;
use indexmap::IndexMap;
use std::mem;

/// A written value with every deletion it carries left out.
///
/// # Errors
///
/// Returns the [`JsonLiteralError`] of a JSON literal that cannot be settled,
/// which no admitted literal raises.
pub(crate) fn settled_value(value: &ExpandedValue<Fragment>) -> Result<ExpandedValue, JsonLiteralError> {
    Ok(match value {
        // clone: the stored value owns its copy of the borrowed fragment.
        ExpandedValue::Literal(literal) => ExpandedValue::Literal(literal.clone()),
        ExpandedValue::Json(json) => ExpandedValue::Json(json.settled()?),
        ExpandedValue::Node(node) => ExpandedValue::Node(settled_node(node)?),
        ExpandedValue::List(items) => ExpandedValue::List(settled_values(items)?),
    })
}

/// Written values, each with its deletions left out, in order.
///
/// # Errors
///
/// As [`settled_value`].
pub(crate) fn settled_values(values: &[ExpandedValue<Fragment>]) -> Result<Vec<ExpandedValue>, JsonLiteralError> {
    values.iter().map(settled_value).collect()
}

/// A written node with its deleted members left out.
fn settled_node(node: &ValueNode<Fragment>) -> Result<ValueNode, JsonLiteralError> {
    let members = node
        .members()
        .iter()
        .filter_map(|(name, member)| match member {
            // clone: the stored node owns its member names.
            FragmentMember::Values(values) => Some(settled_values(values).map(|values| (name.clone(), values))),
            FragmentMember::Remove => None,
        })
        .collect::<Result<_, _>>()?;
    // clone: the stored node owns its identifier and types.
    Ok(ValueNode::new(node.id().cloned(), node.types().to_vec(), members))
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
///
/// # Errors
///
/// As [`settled_value`].
pub(crate) fn merge_value(base: &mut ExpandedValue, patch: &ExpandedValue<Fragment>) -> Result<(), JsonLiteralError> {
    match (base, patch) {
        (ExpandedValue::Json(base_json), ExpandedValue::Json(patch_json)) => *base_json = base_json.merged(patch_json)?,
        // A node carrying `@id` names a resource and is as atomic as a value
        // object; only two anonymous nodes are structures to merge into.
        (ExpandedValue::Node(base_node), ExpandedValue::Node(patch_node)) if base_node.id().is_none() && patch_node.id().is_none() => {
            merge_node(base_node, patch_node)?;
        }
        (base, patch) => *base = settled_value(patch)?,
    }
    Ok(())
}

/// Merge the values of one written member into the stored member: a single
/// value on both sides recurses, any other shape is replaced wholesale.
///
/// # Errors
///
/// As [`settled_value`].
pub(crate) fn merge_values(base: &mut [ExpandedValue], patch: &[ExpandedValue<Fragment>]) -> Result<MemberMerge, JsonLiteralError> {
    match (base, patch) {
        ([base_value], [patch_value]) => {
            merge_value(base_value, patch_value)?;
            Ok(MemberMerge::Merged)
        }
        (_, _) => Ok(MemberMerge::Replace),
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

/// Merge two anonymous structured nodes member by member.
fn merge_node(base: &mut ValueNode, patch: &ValueNode<Fragment>) -> Result<(), JsonLiteralError> {
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
                    if merge_values(existing, values)? == MemberMerge::Replace {
                        *existing = settled_values(values)?;
                    }
                }
                None => {
                    // clone: the stored node owns the member name it gains.
                    base.members_mut().insert(name.clone(), settled_values(values)?);
                }
            },
        }
    }
    Ok(())
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
        language_entries::{LanguageEntries, LanguageEntry},
        node_id::NodeId,
        operation::{
            speed_entity::{member_name, number, text},
            value_merge::{merge_language_entries, merge_value, settled_value},
        },
        value_mode::{Fragment, FragmentMember, FragmentText},
    };
    use andromeda_json_literal::JsonLiteral;
    use indexmap::IndexMap;
    use langtag::LangTagBuf;

    fn literal(text: &str) -> JsonLiteral {
        JsonLiteral::admit(text).expect("a JSON literal")
    }

    fn tag(raw: &str) -> LangTagBuf {
        LangTagBuf::new(raw.to_owned()).expect("a language tag")
    }

    /// A JSON literal merged into nothing loses the keys set to the NGSI-LD
    /// Null string, at any depth of nested objects (IETF RFC 7396).
    #[test]
    fn a_settled_json_literal_leaves_its_deletions_out() {
        let written: ExpandedValue<Fragment> = ExpandedValue::Json(literal(r#"{"a": 1, "b": "urn:ngsi-ld:null", "c": {"d": "urn:ngsi-ld:null", "e": null}}"#));
        pretty_assertions::assert_eq!(
            settled_value(&written).expect("a settled value"),
            ExpandedValue::Json(literal(r#"{"a": 1, "c": {"e": null}}"#))
        );
    }

    /// Two JSON literals merge by IETF RFC 7396.
    #[test]
    fn a_json_literal_merges_into_a_stored_one() {
        let mut held: ExpandedValue = ExpandedValue::Json(literal(r#"{"a": 1, "b": {"c": 2}}"#));
        merge_value(&mut held, &ExpandedValue::Json(literal(r#"{"b": {"c": "urn:ngsi-ld:null", "d": 3}}"#))).expect("a merge");
        pretty_assertions::assert_eq!(held, ExpandedValue::Json(literal(r#"{"a": 1, "b": {"d": 3}}"#)));
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

        merge_value(&mut held, &written).expect("a merge");

        pretty_assertions::assert_eq!(held, settled_value(&written).expect("a settled value"));
    }

    /// A value object meeting a structured node is replaced wholesale.
    #[test]
    fn a_literal_is_replaced_by_the_written_value() {
        let mut held: ExpandedValue = text("old");
        merge_value(&mut held, &number(5)).expect("a merge");
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
