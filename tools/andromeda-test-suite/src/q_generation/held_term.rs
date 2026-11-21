//! A term over one drawn Attribute that the Attribute's value satisfies under the plain reading of clause 4.9.
//!
//! The term is the path that follows the Attribute's name and the comparison it ends with. Some comparisons hold for
//! the drawn value alone, others for every value of a range, an ordering or a pattern the drawn value falls in, so a
//! filter selects the Entity it was drawn from and, as often, other Entities holding the name alike.

use crate::q_generation::{
    drawn_attribute::{Companion, DrawnAttribute, DrawnFamily, PropertyValue},
    member_term::{geometry, json, language, structured, structures},
    scalar_comparison::{Temporal, boolean, double, elements, number, objects, temporal, text, vocabulary},
};
use proptest::prelude::{BoxedStrategy, Just, Strategy, prop_oneof};

/// An operator of clause 4.9.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum Operator {
    /// `==`.
    #[strum(serialize = "==")]
    Equal,
    /// `!=`.
    #[strum(serialize = "!=")]
    Unequal,
    /// `>`.
    #[strum(serialize = ">")]
    Greater,
    /// `>=`.
    #[strum(serialize = ">=")]
    GreaterOrEqual,
    /// `<`.
    #[strum(serialize = "<")]
    Less,
    /// `<=`.
    #[strum(serialize = "<=")]
    LessOrEqual,
    /// `~=`.
    #[strum(serialize = "~=")]
    Matches,
    /// `!~=`.
    #[strum(serialize = "!~=")]
    NotMatches,
}

impl Operator {
    /// The operator of the opposite polarity, which fails where this one holds over a value both compare.
    #[must_use]
    pub const fn flipped(self) -> Operator {
        match self {
            Operator::Equal => Operator::Unequal,
            Operator::Unequal => Operator::Equal,
            Operator::Greater => Operator::LessOrEqual,
            Operator::GreaterOrEqual => Operator::Less,
            Operator::Less => Operator::GreaterOrEqual,
            Operator::LessOrEqual => Operator::Greater,
            Operator::Matches => Operator::NotMatches,
            Operator::NotMatches => Operator::Matches,
        }
    }
}

/// The comparison a term ends with.
#[derive(Debug, Clone, derive_more::Display)]
#[display("{operator}{operand}")]
pub struct Comparison {
    /// The operator.
    pub operator: Operator,
    /// The operand as a client writes it: a value, a range, a value list or a quoted pattern.
    pub operand: String,
}

impl Comparison {
    /// The comparison of the opposite polarity.
    #[must_use]
    pub fn flipped(self) -> Comparison {
        Comparison {
            operator: self.operator.flipped(),
            operand: self.operand,
        }
    }
}

/// The comparison of `operator` with `operand`.
pub fn compared(operator: Operator, operand: &impl ToString) -> Comparison {
    Comparison {
        operator,
        operand: operand.to_string(),
    }
}

/// The request member that must name an Attribute for a term over it to read as drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// None: the term reads the same whatever the request names.
    Nothing,
    /// `jsonKeys`, which keeps the members of a `JsonProperty` literal (clause 4.9).
    JsonKeys,
    /// `expandValues`, which expands the operand against the `@context` as the vocabulary term is.
    ExpandValues,
}

impl Need {
    /// The names `jsonKeys` and `expandValues` list, in that order, for a term over the Attribute `name` that needs
    /// this. Clause 4.9 makes both lists of Attribute names, whichever Entity holds the Attribute, so a term through a
    /// Relationship over an Attribute of the Linked Entity needs its name listed as a term over the Entity's own does.
    #[must_use]
    pub fn listed(self, name: &'static str) -> (Vec<&'static str>, Vec<&'static str>) {
        match self {
            Need::Nothing => (Vec::new(), Vec::new()),
            Need::JsonKeys => (vec![name], Vec::new()),
            Need::ExpandValues => (Vec::new(), vec![name]),
        }
    }
}

/// A term over one Attribute.
#[derive(Debug, Clone)]
pub struct HeldTerm {
    /// What follows the Attribute's name: nothing, a sub-Attribute, or a trailing member path.
    pub path: String,
    /// The comparison, None for a test of existence.
    pub comparison: Option<Comparison>,
    /// The request member that must name the Attribute.
    pub need: Need,
}

impl HeldTerm {
    /// The path of the term over the Attribute `name`.
    #[must_use]
    pub fn path_over(&self, name: &str) -> String {
        format!("{name}{}", self.path)
    }

    /// The comparison as a client writes it, empty for a test of existence.
    #[must_use]
    pub fn comparison_text(&self) -> String {
        self.comparison.as_ref().map_or_else(String::new, Comparison::to_string)
    }
}

/// The term ending `path` with what `comparison` draws, needing nothing beside it.
pub fn ending(path: impl Into<String>, comparison: BoxedStrategy<Comparison>) -> BoxedStrategy<HeldTerm> {
    let path = path.into();
    comparison
        .prop_map(move |comparison| HeldTerm {
            path: path.clone(),
            comparison: Some(comparison),
            need: Need::Nothing,
        })
        .boxed()
}

/// The test that `path` exists, needing nothing beside it.
pub fn existing(path: impl Into<String>) -> BoxedStrategy<HeldTerm> {
    Just(HeldTerm {
        path: path.into(),
        comparison: None,
        need: Need::Nothing,
    })
    .boxed()
}

/// `term` needing `need`.
pub fn needing(term: BoxedStrategy<HeldTerm>, need: Need) -> BoxedStrategy<HeldTerm> {
    term.prop_map(move |term| HeldTerm { need, ..term }).boxed()
}

/// A term the value of a Property satisfies.
fn property_value(value: &PropertyValue) -> BoxedStrategy<HeldTerm> {
    match value {
        PropertyValue::Number(held) => ending("", number(*held)),
        PropertyValue::Text(held) => ending("", text(held)),
        PropertyValue::Boolean(held) => ending("", boolean(*held)),
        PropertyValue::Numbers(held) => match held.as_slice() {
            [single] => ending("", number(*single)),
            several => ending("", elements(several)),
        },
        PropertyValue::Date(held) => ending("", temporal(held, Temporal::Date)),
        PropertyValue::DateTime(held) => ending("", temporal(held, Temporal::DateTime)),
        PropertyValue::Time(held) => ending("", temporal(held, Temporal::Time)),
        PropertyValue::Double(held) => ending("", double(*held)),
        PropertyValue::Structured(members) => structured(members),
        PropertyValue::Structures(objects) => structures(objects),
    }
}

/// A term the companion of a Property satisfies, None for a Property alone.
fn companion(companion: Companion) -> Option<BoxedStrategy<HeldTerm>> {
    match companion {
        Companion::Alone => None,
        Companion::DatasetInstance(second) => Some(ending("", number(second))),
        Companion::SubProperty(sub) => Some(prop_oneof![7 => ending(".sub", number(sub)), 1 => existing(".sub")].boxed()),
        Companion::ObservedAt(instant) => Some(ending(".observedAt", temporal(instant, Temporal::DateTime))),
    }
}

/// A term over the Attribute's own value.
fn own_value(family: &DrawnFamily) -> BoxedStrategy<HeldTerm> {
    match family {
        DrawnFamily::Property(value, held) => match companion(*held) {
            Some(companion) => prop_oneof![3 => property_value(value), 1 => companion].boxed(),
            None => property_value(value),
        },
        DrawnFamily::Geometry(held) => geometry(*held),
        DrawnFamily::Json(held) => needing(json(held), Need::JsonKeys),
        DrawnFamily::Relationship(held) | DrawnFamily::ListRelationship(held) => ending("", objects(held)),
        DrawnFamily::ListProperty(held) => ending("", elements(held)),
        DrawnFamily::Vocab(held) => needing(ending("", vocabulary(held)), Need::ExpandValues),
        DrawnFamily::Language(english, french) => language(english, french),
    }
}

/// A term `attribute` satisfies: most often a comparison its value satisfies, otherwise a test of its existence.
pub fn held_term(attribute: &DrawnAttribute) -> BoxedStrategy<HeldTerm> {
    prop_oneof![1 => existing(""), 40 => own_value(&attribute.family)].boxed()
}
