//! Recognising the NGSI-LD Null in an expanded tree (clause 4.5.0).
//!
//! Expansion leaves the Null a value object (`{"@value": "urn:ngsi-ld:null"}`
//! for a plain term) or a node reference (`{"@id": "urn:ngsi-ld:null"}` under
//! the `@id`- and `@vocab`-typed terms), in either of its two spellings. A
//! family's deletion encoding puts it on the right-hand side of the family's
//! value member (clauses 4.5.2.2, 4.5.3.2, 4.5.18, 4.5.20.2, 4.5.24.2), and a
//! list family's in a list holding it alone (clauses 4.5.21.2, 4.5.22.2), where
//! a normalized `objectList` wraps it in `object` once more. A JSON literal of
//! the Null string is the Null too: the stored document spells it as a value
//! object, which reads back as the Null whatever its datatype.
//!
//! Anywhere else the Null is neither a deletion nor data (clause 4.5.0: "null
//! cannot be used as a value in NGSI-LD"), so beside other values it is
//! refused rather than stored as a value the stored document could not hold.

use crate::expanded_tree::{ExpandedItem, ExpandedNode};
use andromeda_ngsi_ld_entity::{document::value_carrier::ValueCarrier, ngsi_ld_null::is_ngsi_ld_null};
use andromeda_ngsi_ld_vocabulary::expanded::properties;

/// Whether one expanded item is the NGSI-LD Null.
#[must_use]
pub fn is_null(item: &ExpandedItem) -> bool {
    match item {
        ExpandedItem::Value(value) => value.as_str().is_some_and(is_ngsi_ld_null),
        ExpandedItem::Node(node) => node.is_reference() && node.id.as_deref().is_some_and(is_ngsi_ld_null),
        ExpandedItem::Json(literal) => literal.is_null_sentinel(),
        ExpandedItem::List(_) | ExpandedItem::Geometry(_) => false,
    }
}

/// Whether one `objectList` item is the NGSI-LD Null, bare or wrapped in
/// `object`.
#[must_use]
pub fn is_list_object_null(item: &ExpandedItem) -> bool {
    is_null(item) || is_wrapped_null(item)
}

/// Whether a member's values are the NGSI-LD Null alone.
#[must_use]
pub fn is_sole_null(items: &[ExpandedItem]) -> bool {
    matches!(items, [item] if is_null(item))
}

/// Whether the values of the value member `carrier` are the family's deletion
/// encoding.
#[must_use]
pub fn is_deletion(carrier: ValueCarrier, items: &[ExpandedItem]) -> bool {
    match (carrier, items) {
        (ValueCarrier::ValueList | ValueCarrier::ObjectList, [ExpandedItem::List(members)]) => {
            matches!(members.as_slice(), [member] if is_list_object_null(member))
        }
        (ValueCarrier::Json, [ExpandedItem::Json(literal)]) => literal.is_null_sentinel(),
        // Clause 4.5.18 spells the deletion `{"@none": "urn:ngsi-ld:null"}`:
        // the Null under a language deletes that one language of a merged
        // map (clause 5.5.12), not the instance.
        (ValueCarrier::LanguageMap, [item @ ExpandedItem::Value(value)]) => value.language.is_none() && is_null(item),
        (
            ValueCarrier::Value
            | ValueCarrier::Object
            | ValueCarrier::LanguageMap
            | ValueCarrier::Vocab
            | ValueCarrier::Json
            | ValueCarrier::ValueList
            | ValueCarrier::ObjectList,
            items,
        ) => is_sole_null(items),
    }
}

/// Whether a list member is the normalized `{"object": "urn:ngsi-ld:null"}`.
fn is_wrapped_null(member: &ExpandedItem) -> bool {
    match member {
        ExpandedItem::Node(ExpandedNode { id: None, types, members }) if types.is_empty() => {
            matches!(members.as_slice(), [(name, objects)] if name == properties::OBJECT.as_str() && is_sole_null(objects))
        }
        ExpandedItem::Node(_) | ExpandedItem::Value(_) | ExpandedItem::Json(_) | ExpandedItem::List(_) | ExpandedItem::Geometry(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        assembly::null_sentinel::{is_deletion, is_null},
        expanded_tree::{ExpandedItem, ExpandedNode, Scalar, ValueObject},
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_entity::document::value_carrier::ValueCarrier;

    fn literal(text: &str) -> JsonLiteral {
        JsonLiteral::admit(text).expect("a JSON literal")
    }

    fn text(value: &str) -> ExpandedItem {
        ExpandedItem::Value(ValueObject::plain(Scalar::String(value.to_owned())))
    }

    fn reference(id: &str) -> ExpandedItem {
        ExpandedItem::Node(ExpandedNode {
            id: Some(id.to_owned()),
            ..ExpandedNode::default()
        })
    }

    #[test]
    fn both_spellings_are_the_null_as_a_value_and_as_a_reference() {
        for spelling in ["urn:ngsi-ld:null", "https://uri.etsi.org/ngsi-ld/null"] {
            assert!(is_null(&text(spelling)), "{spelling}");
            assert!(is_null(&reference(spelling)), "{spelling}");
        }
        assert!(!is_null(&text("urn:ngsi-ld:nil")));
    }

    /// The stored document spells a JSON literal as a value object, which the
    /// stored-document decoder reads as the Null when it holds the Null string.
    #[test]
    fn a_json_literal_of_the_null_string_is_the_null() {
        assert!(is_null(&ExpandedItem::Json(literal(r#""urn:ngsi-ld:null""#))));
        assert!(!is_null(&ExpandedItem::Json(literal(r#"["urn:ngsi-ld:null"]"#))));
        assert!(!is_null(&ExpandedItem::Json(literal(r#"{"k": "urn:ngsi-ld:null"}"#))));
    }

    /// Clauses 4.5.21.2 and 4.5.22.2: a list family is deleted by a list
    /// holding the Null alone, concise or wrapped in `object`.
    #[test]
    fn a_list_holding_the_null_alone_deletes_a_list_family() {
        let wrapped = ExpandedItem::Node(ExpandedNode {
            members: vec![("https://uri.etsi.org/ngsi-ld/hasObject".to_owned(), vec![reference("urn:ngsi-ld:null")])],
            ..ExpandedNode::default()
        });
        assert!(is_deletion(ValueCarrier::ValueList, &[ExpandedItem::List(vec![text("urn:ngsi-ld:null")])]));
        assert!(is_deletion(ValueCarrier::ObjectList, &[ExpandedItem::List(vec![wrapped])]));
        assert!(!is_deletion(
            ValueCarrier::ValueList,
            &[ExpandedItem::List(vec![text("urn:ngsi-ld:null"), text("data")])]
        ));
    }

    /// Clause 4.5.24.2: a JSON literal that is the Null string deletes a
    /// `JsonProperty`; a JSON `null` is data.
    #[test]
    fn a_json_literal_of_the_null_string_deletes_a_json_property() {
        assert!(is_deletion(ValueCarrier::Json, &[ExpandedItem::Json(literal(r#""urn:ngsi-ld:null""#))]));
        assert!(!is_deletion(ValueCarrier::Json, &[ExpandedItem::Json(literal("null"))]));
    }
}
