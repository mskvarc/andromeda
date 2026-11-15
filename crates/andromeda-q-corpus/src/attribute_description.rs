//! The typed description of a corpus Entity's Attributes, which a consumer
//! turns into whatever form it evaluates over.
//!
//! The description is the clause 5.4 data model, not a wire format: an
//! Attribute holds instances, an instance one family's value and the members
//! clause 4.9 reads, so each execution builds its own representation from it.

use crate::corpus_entity::CorpusEntity;

/// One Attribute of a corpus Entity, by the short term the requester's
/// `@context` gives its name.
#[derive(Clone, Copy, Debug)]
pub struct CorpusAttribute {
    /// The short term naming the Attribute.
    pub term: &'static str,
    /// The instances of the Attribute, the default instance first.
    pub instances: &'static [CorpusInstance],
}

/// One instance of an Attribute.
#[derive(Clone, Copy, Debug)]
pub struct CorpusInstance {
    /// The family of the instance and what it holds.
    pub content: CorpusContent,
    /// The `datasetId` of the instance, none for the default instance.
    pub dataset_id: Option<&'static str>,
    /// The `observedAt` of the instance as written, if it has one.
    pub observed_at: Option<&'static str>,
    /// The `unitCode` of the instance, if it has one.
    pub unit_code: Option<&'static str>,
    /// The Attributes of the instance, which clause 4.9 reaches with `.`.
    pub sub_attributes: &'static [CorpusAttribute],
}

/// The family of an instance and the value it holds (clause 5.2.4).
#[derive(Clone, Copy, Debug)]
pub enum CorpusContent {
    /// A Property, whose value is an array when several values are written.
    Property(&'static [CorpusValue]),
    /// A Relationship to one or more objects.
    Relationship(&'static [CorpusTarget]),
    /// A `ListRelationship` to its objects in order.
    ListRelationship(&'static [CorpusTarget]),
    /// A `LanguageProperty` holding one string per language tag.
    LanguageProperty(&'static [CorpusTranslation]),
    /// A `VocabProperty` holding one term.
    VocabProperty(&'static str),
    /// A `ListProperty` holding its items in order.
    ListProperty(&'static [CorpusValue]),
    /// A `JsonProperty` holding the JSON text as written.
    JsonProperty(&'static str),
    /// A `GeoProperty` holding a Point.
    GeoProperty {
        /// The longitude of the Point, the first coordinate.
        longitude: f64,
        /// The latitude of the Point, the second coordinate.
        latitude: f64,
        /// The `bbox` written beside the Point (IETF RFC 7946 section 5), its
        /// west, south, east and north bounds, if one is.
        bbox: Option<[f64; 4]>,
        /// The foreign members of the Point (RFC 7946 section 6.1), each keyed
        /// by its short term with the values it holds, as the members of a
        /// structured value are.
        foreign: &'static [(&'static str, &'static [CorpusValue])],
    },
}

/// One value of a Property or a `ListProperty`.
#[derive(Clone, Copy, Debug)]
pub enum CorpusValue {
    /// An integer Number.
    Integer(i64),
    /// A String.
    Text(&'static str),
    /// A Boolean.
    Boolean(bool),
    /// A String declared as a value of a data type.
    Declared {
        /// The data type the lexical form is declared as.
        datatype: CorpusDatatype,
        /// The lexical form of the value.
        lexical: &'static str,
    },
    /// A structured value, each member keyed by its short term with the
    /// values it holds: one written alone, none or several written as an
    /// array.
    Members(&'static [(&'static str, &'static [CorpusValue])]),
}

/// A data type a corpus value is declared as.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorpusDatatype {
    /// An NGSI-LD Date (clause 4.6.3).
    Date,
    /// An NGSI-LD `DateTime` (clause 4.6.3).
    DateTime,
    /// An NGSI-LD Time (clause 4.6.3).
    Time,
    /// An `xsd:double`.
    Double,
}

/// The object of a Relationship or a `ListRelationship`.
#[derive(Clone, Copy, Debug)]
pub enum CorpusTarget {
    /// An identifier no corpus Entity has, as written.
    Identifier(&'static str),
    /// A corpus Entity, whose identifier the consumer assigns.
    Entity(CorpusEntity),
}

/// One entry of a language map.
#[derive(Clone, Copy, Debug)]
pub struct CorpusTranslation {
    /// The language tag of the entry.
    pub language: &'static str,
    /// The string of the entry.
    pub text: &'static str,
}
