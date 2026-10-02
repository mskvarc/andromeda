//! One item of an expanded property, in the shape compaction tells apart.
//!
//! Every array of an expanded document holds value objects, node objects
//! (node references among them) and list objects (JSON-LD 1.1 section 9).
//! Compaction selects a term per item from that shape and compacts the item by
//! it, so the walk first states what each piece of the model is in those
//! terms: a scalar becomes the library's own value object, an identifier its
//! own `@id`, and the larger nodes -- attribute instances, geometries,
//! structured values -- stay borrowed from the model and are walked in place.

use crate::{compacted::Compacted, compacted_mode::CompactedMode, undecidable::Undecidable};
use andromeda_ngsi_ld_entity::{
    document::instance_document::InstanceDocument,
    expanded_value::{ExpandedValue, ValueNode},
    geometry_value::GeometryValue,
    instance::Instance,
    json_literal::JsonLiteral,
    literal::{Direction as LiteralDirection, Literal as ModelLiteral, LiteralValue},
    value_mode::ValueMode,
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, uri::Uri};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use jsonld::{
    BlankIdBuf,
    Direction,
    Id,
    IriBuf,
    LangString,
    Value,
    compaction::ValueKind,
    object::Literal,
    syntax::{LenientLangTagBuf, NumberBuf, Value as JsonValue},
};

/// A JSON-LD node identifier.
pub(crate) type NodeIdentifier = Id<IriBuf, BlankIdBuf>;

/// One stored attribute instance with the members it is written with.
pub(crate) struct StoredInstance<'a> {
    /// The instance.
    pub(crate) instance: &'a Instance,
    /// The `datasetId` it is held under.
    pub(crate) dataset_id: Option<&'a Identifier>,
    /// Whether its clause 4.8 `createdAt` and `modifiedAt` are written.
    pub(crate) system_times: SysAttrsInclusion,
}

/// One item of an expanded property.
pub(crate) enum ExpandedItem<'a, M: ValueMode> {
    /// A value object.
    Value(Value<IriBuf>),
    /// A node reference: a node object holding only an `@id`.
    Reference(NodeIdentifier),
    /// A node of a structured value, with its `@id` when it has one.
    Node(&'a ValueNode<M>, Option<NodeIdentifier>),
    /// A list object.
    List(Vec<ExpandedItem<'a, M>>),
    /// A stored attribute instance node.
    StoredInstance(StoredInstance<'a>),
    /// A written attribute instance node.
    WrittenInstance(&'a InstanceDocument),
    /// A geometry node of the `GeoJSON` vocabulary.
    Geometry(&'a GeometryValue),
    /// A `ListRelationship` item in its normalized spelling, a node holding
    /// one `hasObject` reference (clause 4.5.22.2).
    WrappedObject(&'a Uri),
}

impl<'a, M: CompactedMode> ExpandedItem<'a, M> {
    /// The item a model value stands for.
    ///
    /// # Errors
    ///
    /// Returns [`Undecidable::UnrepresentableValue`] for a value with no
    /// JSON-LD form the walk can build.
    pub(crate) fn of_value(value: &'a ExpandedValue<M>) -> Result<ExpandedItem<'a, M>, Undecidable> {
        match value {
            ExpandedValue::Literal(literal) => literal_value(literal).map(ExpandedItem::Value),
            ExpandedValue::Json(json) => Ok(ExpandedItem::Value(json_value(json))),
            ExpandedValue::Node(node) => Ok(match node.id() {
                Some(id) if node.is_reference() => ExpandedItem::Reference(node_identifier(id.as_str())),
                Some(id) => ExpandedItem::Node(node, Some(node_identifier(id.as_str()))),
                None => ExpandedItem::Node(node, None),
            }),
            ExpandedValue::List(items) => items.iter().map(ExpandedItem::of_value).collect::<Result<Vec<_>, _>>().map(ExpandedItem::List),
        }
    }

    /// The items of a run of model values.
    pub(crate) fn of_values(values: &'a [ExpandedValue<M>]) -> Result<Vec<ExpandedItem<'a, M>>, Undecidable> {
        values.iter().map(ExpandedItem::of_value).collect()
    }
}

impl<M: ValueMode> ExpandedItem<'_, M> {
    /// A plain string value object.
    pub(crate) fn text(text: &str) -> Self {
        ExpandedItem::Value(Value::Literal(Literal::String(text.into()), None))
    }

    /// A string value object of `datatype`.
    pub(crate) fn typed_text(text: &str, datatype: IriBuf) -> Self {
        ExpandedItem::Value(Value::Literal(Literal::String(text.into()), Some(datatype)))
    }

    /// A string value object in `language`.
    pub(crate) fn tagged_text(text: &str, language: &str) -> Self {
        let tagged = LangString::with_language(text.into(), LenientLangTagBuf::from(language.to_owned()));
        ExpandedItem::Value(Value::LangString(tagged))
    }

    /// A `@json` value object.
    pub(crate) fn json(json: &JsonLiteral) -> Self {
        ExpandedItem::Value(json_value(json))
    }

    /// A node reference to `iri`.
    pub(crate) fn reference(iri: &str) -> Self {
        ExpandedItem::Reference(node_identifier(iri))
    }

    /// The shape term selection reads off the item (JSON-LD 1.1 API section
    /// 6.2.2). A list is described by its items' shapes where it is selected
    /// for; as an item of another list only its being no value object counts.
    pub(crate) fn kind(&self) -> ValueKind<'_, IriBuf, BlankIdBuf> {
        match self {
            ExpandedItem::Value(value) => value_kind(value),
            ExpandedItem::Reference(id) => ValueKind::Node { id: Some(id) },
            ExpandedItem::Node(_, id) => ValueKind::Node { id: id.as_ref() },
            ExpandedItem::List(_) => ValueKind::List(&[]),
            ExpandedItem::StoredInstance(_) | ExpandedItem::WrittenInstance(_) | ExpandedItem::Geometry(_) | ExpandedItem::WrappedObject(_) => {
                ValueKind::Node { id: None }
            }
        }
    }
}

/// The shape of a value object.
pub(crate) fn value_kind(value: &Value<IriBuf>) -> ValueKind<'_, IriBuf, BlankIdBuf> {
    match value {
        Value::Literal(_, datatype) => ValueKind::Literal { datatype: datatype.as_ref() },
        Value::LangString(string) => ValueKind::LangString {
            language: string.language(),
            direction: string.direction(),
        },
        Value::Json(_) => ValueKind::Json,
    }
}

/// The JSON a value object carries under `@value`, without its type,
/// language or direction: what a language map holds per language.
pub(crate) fn scalar_of<'a>(value: &Value<IriBuf>) -> Compacted<'a> {
    // clone: the language map owns the scalar it holds, the item keeps its
    // value object
    match value {
        Value::Literal(literal, _) => match literal {
            Literal::Null => Compacted::Null,
            Literal::Boolean(flag) => Compacted::Boolean(*flag),
            Literal::Number(number) => Compacted::Number(number.clone()),
            Literal::String(text) => Compacted::String(Box::from(text.as_str())),
        },
        Value::LangString(string) => Compacted::String(Box::from(string.as_str())),
        Value::Json(json) => Compacted::from(json.clone()),
    }
}

/// The `@id` of a node, read as the processor reads one: an IRI, a blank node
/// identifier, or a string kept as an invalid identifier.
pub(crate) fn node_identifier(text: &str) -> NodeIdentifier {
    Id::from_string(text.to_owned())
}

/// The library's value object for a model literal.
fn literal_value(literal: &ModelLiteral) -> Result<Value<IriBuf>, Undecidable> {
    let datatype = literal
        .datatype()
        .map(|datatype| IriBuf::new(datatype.as_str().to_owned()).map_err(|_| Undecidable::UnrepresentableValue))
        .transpose()?;
    let scalar = match literal.value() {
        LiteralValue::Number(number) => Literal::Number(number_buf(number)?),
        LiteralValue::Boolean(flag) => Literal::Boolean(*flag),
        LiteralValue::String(text) => {
            let language = literal.language().map(|language| LenientLangTagBuf::from(language.as_str().to_owned()));
            let direction = literal.direction().map(direction);
            if language.is_some() || direction.is_some() {
                return LangString::new(text.as_str().into(), language, direction)
                    .map(Value::LangString)
                    .map_err(|_| Undecidable::UnrepresentableValue);
            }
            Literal::String(text.as_str().into())
        }
    };
    Ok(Value::Literal(scalar, datatype))
}

/// The library's `@json` value object for a JSON literal.
fn json_value(json: &JsonLiteral) -> Value<IriBuf> {
    // clone: the library's value object owns its JSON document
    Value::Json(JsonValue::from_serde_json(json.get().clone()))
}

/// A number in the exact spelling the canonical codec writes it with.
fn number_buf(number: &JsonNumber) -> Result<NumberBuf, Undecidable> {
    serde_json::to_value(number)
        .and_then(serde_json::from_value::<serde_json::Number>)
        .map(NumberBuf::from)
        .map_err(|_| Undecidable::UnrepresentableValue)
}

/// The library's base direction for a model one.
const fn direction(direction: LiteralDirection) -> Direction {
    match direction {
        LiteralDirection::Ltr => Direction::Ltr,
        LiteralDirection::Rtl => Direction::Rtl,
    }
}
