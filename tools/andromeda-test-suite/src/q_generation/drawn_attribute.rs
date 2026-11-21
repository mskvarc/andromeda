//! The Attributes a drawn Entity holds, every attribute family among them, and the instances each one writes.

use crate::{geometry_round_trip::line, q_generation::drawn_pool::POSITIONS, value::xsd_typed};
use googletest::prelude::*;
use indexmap::IndexMap;
use ngsi_ld::model::{
    Attribute,
    AttributeDraft,
    CoreDatatype,
    DatasetId,
    DateTime,
    EntityId,
    Input,
    JsonProperty,
    LanguageMap,
    ListProperty,
    ListRelationship,
    Number,
    Property,
    RawJson,
    Relationship,
    TypedValue,
    Value,
    VocabProperty,
    VocabTerm,
    geometry::{Position, Shape},
};

/// What a member of a drawn JSON object holds.
#[derive(Debug, Clone)]
pub enum MemberValue {
    /// A number.
    Number(u8),
    /// A string.
    Text(&'static str),
    /// An array of numbers, possibly empty.
    Numbers(Vec<u8>),
    /// An object holding the one member of this name and number.
    Nested(&'static str, u8),
    /// A `DateTime`, declared in a Property's value and a plain string in the JSON of a `JsonProperty`.
    DateTime(&'static str),
}

/// One member of a drawn JSON object.
#[derive(Debug, Clone)]
pub struct Member<V> {
    /// Its name.
    pub name: &'static str,
    /// What it holds.
    pub value: V,
}

/// The JSON a drawn `JsonProperty` holds, an object or an array of objects as clause 4.5.24.2 admits; a member holding
/// `None` holds the JSON `null`.
#[derive(Debug, Clone)]
pub enum DrawnJson {
    /// An object, its members named from the pool of JSON member names.
    Object(Vec<Member<Option<MemberValue>>>),
    /// An array of objects.
    Objects(Vec<Vec<Member<Option<MemberValue>>>>),
}

/// The geometry a drawn `GeoProperty` holds, through positions of [`POSITIONS`] named by index.
#[derive(Debug, Clone, Copy)]
pub enum DrawnGeometry {
    /// A Point.
    Point(usize),
    /// A `LineString` between two distinct positions.
    Line(usize, usize),
}

/// The value a drawn Property holds.
#[derive(Debug, Clone)]
pub enum PropertyValue {
    /// A number.
    Number(u8),
    /// A string.
    Text(&'static str),
    /// A Boolean.
    Boolean(bool),
    /// An array of numbers.
    Numbers(Vec<u8>),
    /// A string declared an NGSI-LD Date.
    Date(&'static str),
    /// A string declared an NGSI-LD `DateTime`.
    DateTime(&'static str),
    /// A string declared an NGSI-LD Time.
    Time(&'static str),
    /// An `xsd:double` lexical form, not-a-number and the infinities among them.
    Double(&'static str),
    /// A JSON object, its members named from the pool of structure member names.
    Structured(Vec<Member<MemberValue>>),
    /// An array of JSON objects.
    Structures(Vec<Vec<Member<MemberValue>>>),
}

/// What a drawn Property carries beside its value.
#[derive(Debug, Clone, Copy)]
pub enum Companion {
    /// Nothing.
    Alone,
    /// A second instance in a dataset, holding this number.
    DatasetInstance(u8),
    /// A sub-property `sub` holding this number.
    SubProperty(u8),
    /// The observation time `observedAt`.
    ObservedAt(&'static str),
}

/// The family of a drawn Attribute and what it holds.
#[derive(Debug, Clone)]
pub enum DrawnFamily {
    /// A Property.
    Property(PropertyValue, Companion),
    /// A `GeoProperty`.
    Geometry(DrawnGeometry),
    /// A `JsonProperty`.
    Json(DrawnJson),
    /// A Relationship with one object or several distinct ones.
    Relationship(Vec<&'static str>),
    /// A `ListRelationship`.
    ListRelationship(Vec<&'static str>),
    /// A `ListProperty` of numbers.
    ListProperty(Vec<u8>),
    /// A `VocabProperty` holding a term.
    Vocab(&'static str),
    /// A `LanguageProperty` with an English and a French string.
    Language(&'static str, &'static str),
}

/// One drawn Attribute.
#[derive(Debug, Clone)]
pub struct DrawnAttribute {
    /// Its name, from the pool of Attribute names.
    pub name: &'static str,
    /// Its family and what it holds.
    pub family: DrawnFamily,
}

impl DrawnAttribute {
    /// The default instance the Attribute writes, and a second one in `dataset` when its companion asks for one.
    ///
    /// # Errors
    ///
    /// Fails when a drawn value does not build.
    pub fn instances(&self, dataset: &DatasetId) -> Result<Vec<Attribute<Input>>> {
        Ok(match &self.family {
            DrawnFamily::Property(value, companion) => property_instances(value, *companion, dataset)?,
            DrawnFamily::Geometry(geometry) => vec![Attribute::GeoProperty(geometry.shape()?.into_attribute()?)],
            DrawnFamily::Json(json) => vec![Attribute::JsonProperty(JsonProperty::builder(json.text().parse::<RawJson>()?).build()?)],
            DrawnFamily::Relationship(objects) => vec![Attribute::Relationship(match object_list(objects)?.as_slice() {
                [object] => Relationship::builder(object.clone()).build()?,
                several => Relationship::builder(several.to_vec()).build()?,
            })],
            DrawnFamily::ListRelationship(objects) => vec![Attribute::ListRelationship(ListRelationship::builder(object_list(objects)?).build()?)],
            DrawnFamily::ListProperty(values) => vec![Attribute::ListProperty(ListProperty::builder(number_list(values)?).build()?)],
            DrawnFamily::Vocab(term) => vec![Attribute::VocabProperty(VocabProperty::builder(term.parse::<VocabTerm>()?).build()?)],
            DrawnFamily::Language(english, french) => vec![Attribute::LanguageProperty(
                LanguageMap::try_from_pairs([("en", *english), ("fr", *french)])?.into_attribute()?,
            )],
        })
    }
}

/// The instances a Property holding `value` with `companion` writes.
fn property_instances(value: &PropertyValue, companion: Companion, dataset: &DatasetId) -> Result<Vec<Attribute<Input>>> {
    let observed_at = match companion {
        Companion::ObservedAt(instant) => Some(instant.parse::<DateTime>()?),
        Companion::Alone | Companion::DatasetInstance(_) | Companion::SubProperty(_) => None,
    };
    let draft = Property::builder(value.value()?)
        .maybe_value_type(value.declared_type()?)
        .maybe_observed_at(observed_at);
    let draft = match companion {
        Companion::SubProperty(sub) => draft.property("sub", number(sub)?),
        Companion::Alone | Companion::DatasetInstance(_) | Companion::ObservedAt(_) => draft,
    };
    let mut instances = vec![Attribute::Property(draft.build()?)];
    if let Companion::DatasetInstance(second) = companion {
        instances.push(Attribute::Property(Property::builder(number(second)?).dataset_id(dataset.clone()).build()?));
    }
    Ok(instances)
}

impl PropertyValue {
    /// The value the Property holds.
    fn value(&self) -> Result<Value> {
        Ok(match self {
            PropertyValue::Number(value) => number(*value)?,
            PropertyValue::Text(text) | PropertyValue::Date(text) | PropertyValue::DateTime(text) | PropertyValue::Time(text) => Value::from(*text),
            PropertyValue::Boolean(flag) => Value::from(*flag),
            PropertyValue::Numbers(values) => Value::Array(number_list(values)?),
            PropertyValue::Double(lexical) => xsd_typed("double", lexical)?,
            PropertyValue::Structured(members) => structure(members)?,
            PropertyValue::Structures(objects) => Value::Array(objects.iter().map(|members| structure(members)).collect::<Result<_>>()?),
        })
    }

    /// The `valueType` that declares the value a time, for a declared Date, `DateTime` or Time.
    fn declared_type(&self) -> Result<Option<VocabTerm>> {
        let declared = match self {
            PropertyValue::Date(_) => "Date",
            PropertyValue::DateTime(_) => "DateTime",
            PropertyValue::Time(_) => "Time",
            PropertyValue::Number(_)
            | PropertyValue::Text(_)
            | PropertyValue::Boolean(_)
            | PropertyValue::Numbers(_)
            | PropertyValue::Double(_)
            | PropertyValue::Structured(_)
            | PropertyValue::Structures(_) => return Ok(None),
        };
        Ok(Some(declared.parse()?))
    }
}

impl MemberValue {
    /// The member as a Property value holds it.
    fn value(&self) -> Result<Value> {
        Ok(match self {
            MemberValue::Number(value) => number(*value)?,
            MemberValue::Text(text) => Value::from(*text),
            MemberValue::Numbers(values) => Value::Array(number_list(values)?),
            MemberValue::Nested(name, value) => Value::Object(IndexMap::from([((*name).to_owned(), number(*value)?)])),
            MemberValue::DateTime(instant) => Value::Typed(TypedValue::try_new(CoreDatatype::DateTime, *instant)?),
        })
    }

    /// The member as JSON text.
    fn json(&self) -> String {
        match self {
            MemberValue::Number(value) => value.to_string(),
            MemberValue::Text(text) | MemberValue::DateTime(text) => format!("\"{text}\""),
            MemberValue::Numbers(values) => format!("[{}]", values.iter().map(u8::to_string).collect::<Vec<_>>().join(",")),
            MemberValue::Nested(name, value) => format!("{{\"{name}\":{value}}}"),
        }
    }
}

/// `value` as JSON text, `None` as the JSON `null`.
fn json_member(value: Option<&MemberValue>) -> String {
    value.map_or_else(|| "null".to_owned(), MemberValue::json)
}

/// The JSON object with `members` as text.
fn json_object(members: &[Member<Option<MemberValue>>]) -> String {
    format!(
        "{{{}}}",
        members
            .iter()
            .map(|member| format!("\"{}\":{}", member.name, json_member(member.value.as_ref())))
            .collect::<Vec<_>>()
            .join(",")
    )
}

impl DrawnJson {
    /// The JSON as text.
    fn text(&self) -> String {
        match self {
            DrawnJson::Object(members) => json_object(members),
            DrawnJson::Objects(objects) => format!("[{}]", objects.iter().map(|members| json_object(members)).collect::<Vec<_>>().join(",")),
        }
    }
}

impl DrawnGeometry {
    /// The geometry as a shape.
    fn shape(self) -> Result<Shape> {
        Ok(match self {
            DrawnGeometry::Point(at) => Shape::Point(position(at)?),
            DrawnGeometry::Line(from, to) => Shape::LineString(line(&[coordinates(from)?, coordinates(to)?])?),
        })
    }
}

/// The longitude and latitude of the position of [`POSITIONS`] at `index`.
fn coordinates(index: usize) -> Result<(f64, f64)> {
    POSITIONS.get(index).copied().or_fail()
}

/// The position of [`POSITIONS`] at `index`.
fn position(index: usize) -> Result<Position> {
    let (longitude, latitude) = coordinates(index)?;
    Ok(Position::try_new(longitude, latitude)?)
}

/// The JSON object with `members`, as a Property value holds it.
fn structure(members: &[Member<MemberValue>]) -> Result<Value> {
    Ok(Value::Object(
        members
            .iter()
            .map(|member| Ok((member.name.to_owned(), member.value.value()?)))
            .collect::<Result<IndexMap<_, _>>>()?,
    ))
}

/// The objects `objects` as Entity identifiers.
fn object_list(objects: &[&str]) -> Result<Vec<EntityId>> {
    let mut identifiers = Vec::with_capacity(objects.len());
    for object in objects {
        identifiers.push(object.parse()?);
    }
    Ok(identifiers)
}

/// The number `value` as a Value.
fn number(value: u8) -> Result<Value> {
    Ok(Value::Number(value.to_string().parse::<Number>()?))
}

/// The numbers `values` as Values.
fn number_list(values: &[u8]) -> Result<Vec<Value>> {
    values.iter().map(|value| number(*value)).collect()
}
