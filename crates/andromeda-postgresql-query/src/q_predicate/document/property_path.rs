//! Where a Query Term's target lives in a document of Properties: the node of
//! the last (sub-)Property its path names, the value that node holds, and the
//! compound member a trailing group steps to.
//!
//! A document holds each property as its writer wrote it, and reads it as the
//! in-memory evaluator reads a registration's Context Source Properties: a
//! JSON object holding a `value` member, or naming an NGSI-LD attribute family
//! by `type`, is the property's node, whose value is its `value` member, whose
//! instance members are read under the terms clause 4.5.2.2 writes them by,
//! and whose other members, but `type` and those any family defines, are its
//! sub-properties (clause 4.5.2.3), and any other JSON, an array included, is
//! the value itself, which has no sub-property.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, time::value::TimeValueForm};
use andromeda_ngsi_ld_query::filter::{
    instance_metadata::InstanceMetadata,
    reading::{
        family_reading::{ValueKeys, family_reading},
        operand::Operand,
        term_reading::TermReading,
        terminal::Terminal,
    },
};
use andromeda_ngsi_ld_vocabulary::{
    compact::{
        classes::{DATE, DATE_TIME, TIME},
        properties as compact_properties,
    },
    keyword,
};
use andromeda_postgresql_seaquery::{
    cast::typed_null,
    column_reference::ColumnReference,
    datetime::pg_input_is_valid,
    json_type::{JsonType, jsonb_type_is, jsonb_whole_text},
    literal::text_literal,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{Alias, CaseStatement, Expr as SeaExpr, ExprTrait, Query, QueryStatementBuilder, SimpleExpr};

/// The family every property of a document is: clause 5.2.9 makes Context
/// Source Properties Properties, never a `LanguageProperty`.
const DOCUMENT_FAMILY: AttributeType = AttributeType::Property;

/// The path of Properties a Query Term reads in a document, from the property
/// it names to the target it ends on.
#[derive(Debug, Clone, Copy)]
pub struct PropertyPath<'a> {
    /// The property the path starts at, a member of the document.
    attribute: &'a AttributeName,
    /// The sub-properties below it, each a member of the node above.
    sub_properties: &'a [AttributeName],
    /// What the path ends on below the last of them.
    ending: Ending<'a>,
}

/// What a property path ends on at its last (sub-)Property.
#[derive(Debug, Clone, Copy)]
enum Ending<'a> {
    /// The node itself.
    Node,
    /// A member the node carries under the term clause 4.5.2.2 writes it by,
    /// read as one more sub-property: a document holds a property's members as
    /// its writer keyed them.
    InstanceMember(InstanceMetadata),
    /// The members of a compound value, keys of the node's value.
    Members(&'a [AttributeName]),
}

impl<'a> PropertyPath<'a> {
    /// The path `term` reads in a document, `None` when it has no target
    /// there.
    ///
    /// The clause 4.9 `{}` axis descends a Relationship into the entity it
    /// names, and Context Source Properties carry no Relationship, so a term
    /// with a hop has no target; nor does a target language, which only a
    /// `LanguageProperty` holds. Nor does a `.` naming a member a node carries
    /// for itself under any family (`value`, `type`, `object`, `unitCode`,
    /// ...), which clause 4.5.1 bars as an Attribute name. A term without a
    /// target satisfies no document, though it is valid `q` syntax.
    #[must_use]
    pub fn of(term: &'a TermReading) -> Option<PropertyPath<'a>> {
        if !term.hops().is_empty() || term.sub_attributes().iter().any(|sub| AttributeType::is_own_member(sub.as_str())) {
            return None;
        }
        let ending = match term.terminal() {
            Terminal::OwnValue => Ending::Node,
            Terminal::InstanceMember(member) => Ending::InstanceMember(*member),
            Terminal::Members(members) => Ending::Members(&members.addressed),
            Terminal::ByFamily { members, .. } => match family_reading(DOCUMENT_FAMILY).keys {
                ValueKeys::Members => Ending::Members(&members.addressed),
                ValueKeys::Languages => return None,
            },
            Terminal::Language { .. } => return None,
        };
        Some(PropertyPath {
            attribute: term.attribute(),
            sub_properties: term.sub_attributes(),
            ending,
        })
    }

    /// The target element an existence term asks `document` for, NULL when
    /// it is absent: the node the path ends on, the text of the instance
    /// member it ends on, or the compound member its trailing group steps to.
    ///
    /// Clause 4.9 makes an existence term select what defines the target
    /// element "regardless of any target value", so a node with no value is
    /// present, and a member is present when the value holds it. An instance
    /// member is a string, so one holding other JSON is not present.
    #[must_use]
    pub fn element(self, document: &ColumnReference) -> SimpleExpr {
        match self.ending {
            Ending::Node => self.node(document),
            Ending::InstanceMember(_) | Ending::Members(_) => self.value(document),
        }
    }

    /// The target value the path names in `document`, NULL when it is absent:
    /// the value its last node holds, each compound member stepped into that
    /// value, or the text of the instance member it ends on.
    ///
    /// A compound member is an ordinary JSON key of the value, not a property
    /// node of its own, so it is stepped as it is: a member of a compound
    /// value holding `value` is read whole.
    #[must_use]
    pub fn value(self, document: &ColumnReference) -> SimpleExpr {
        match self.ending {
            Ending::Node => held_value_of(self.node(document)),
            Ending::InstanceMember(_) => instance_member_text(self.node(document)),
            Ending::Members(members) => members.iter().fold(held_value_of(self.node(document)), |value, member| {
                value.json_get(text_literal(member.as_str()))
            }),
        }
    }

    /// The JSONB value and the declared datatype IRI the target `value` reads
    /// is compared with `operand` through.
    ///
    /// An instance member holding an instant (clause 4.8, clause 4.22) is the
    /// `DateTime` its text spells, and a String when it spells none, as the
    /// in-memory evaluator reads it. Any other target is read by
    /// [`declared_value_reading`].
    pub fn declared_reading(self, value: impl Fn() -> SimpleExpr, operand: &Operand) -> (SimpleExpr, SimpleExpr) {
        match self.ending {
            Ending::InstanceMember(member) if member.holds_instant() => {
                let spells_an_instant = pg_input_is_valid(jsonb_whole_text(value()), PostgresType::Timestamptz);
                let datatype = CaseStatement::new().case(spells_an_instant, text_literal(TimeValueForm::DateTime.iri().as_str()));
                (value(), datatype.into())
            }
            Ending::InstanceMember(_) => (value(), typed_null(PostgresType::Text)),
            Ending::Node | Ending::Members(_) => declared_value_reading(value, operand),
        }
    }

    /// The node of the last (sub-)Property the path names in `document`.
    ///
    /// Clause 4.9 makes each name after the first of a path "a (sub)Property
    /// or (sub)Relationship", and a sub-property is a member of its property's
    /// node, beside the node's value; a property written as its value alone
    /// has none.
    fn node(self, document: &ColumnReference) -> SimpleExpr {
        let node = self
            .sub_properties
            .iter()
            .fold(document.column_expr().json_get(text_literal(self.attribute.as_str())), |node, sub| {
                node_member(node, sub.as_str())
            });
        match self.ending {
            Ending::InstanceMember(member) => node_member(node, member.term()),
            Ending::Node | Ending::Members(_) => node,
        }
    }
}

/// The alias a node is read under inside the scalar subquery that reads it
/// once.
const NODE: &str = "j";

/// The node a scalar subquery reads under [`NODE`].
fn aliased_node() -> SimpleExpr {
    SeaExpr::col(Alias::new(NODE))
}

/// `read` over `node`, which a scalar subquery aliases as [`NODE`], so `read`
/// names a column as often as it needs rather than embedding the node
/// expression each time.
fn read_once(node: SimpleExpr, read: SimpleExpr) -> SimpleExpr {
    let derived = Query::select().expr_as(node, Alias::new(NODE)).to_owned();
    let read = Query::select().expr(read).from_subquery(derived, Alias::new("s")).to_owned();
    SimpleExpr::SubQuery(None, Box::new(read.into_sub_query_statement()))
}

/// Whether the aliased JSON is a property's node: an object holding `value`,
/// or naming an NGSI-LD attribute family by `type`.
fn is_node() -> SimpleExpr {
    let families = AttributeType::ALL.into_iter().map(|family| text_literal(<&'static str>::from(family)));
    SeaExpr::expr(aliased_node().json_get(text_literal(compact_properties::VALUE)))
        .is_not_null()
        .or(SeaExpr::expr(aliased_node().json_get_text(text_literal(keyword::TYPE_ALIAS))).is_in(families))
}

/// The value the aliased JSON holds: the `value` member of a node, NULL for a
/// node holding none, or the JSON itself when it is no node.
fn held_value() -> SimpleExpr {
    CaseStatement::new()
        .case(is_node(), aliased_node().json_get(text_literal(compact_properties::VALUE)))
        .finally(aliased_node())
        .into()
}

/// The member `name` of the node `node` is, NULL when `node` is no node,
/// reading `node` exactly once.
fn node_member(node: SimpleExpr, name: &str) -> SimpleExpr {
    read_once(node, CaseStatement::new().case(is_node(), aliased_node().json_get(text_literal(name))).into())
}

/// The value `node` holds, reading `node` exactly once.
fn held_value_of(node: SimpleExpr) -> SimpleExpr {
    read_once(node, held_value())
}

/// The value the instance member `node` holds when it is a JSON string, NULL
/// otherwise, reading `node` exactly once.
fn instance_member_text(node: SimpleExpr) -> SimpleExpr {
    read_once(
        node,
        CaseStatement::new().case(jsonb_type_is(held_value(), JsonType::String), held_value()).into(),
    )
}

/// The JSONB value and the declared datatype IRI a stored `value` is compared
/// with `operand` through.
///
/// Only a value object declaring a time value form in its `@type` compares
/// with a time operand (clause 4.9: no equality of data types, no match), so
/// such an operand reads the object's `@value` beside its datatype. A document
/// keeps the value its writer stored, so that `@type` is the core context's
/// term for the form or the form's IRI. Every other operand reads the whole
/// value and no datatype.
fn declared_value_reading(value: impl Fn() -> SimpleExpr, operand: &Operand) -> (SimpleExpr, SimpleExpr) {
    match operand {
        Operand::DateTime(_) | Operand::Date(_) | Operand::Time(_) => {
            let datatype = || value().json_get_text(text_literal(keyword::TYPE));
            let declared = [(TimeValueForm::DateTime, DATE_TIME), (TimeValueForm::Date, DATE), (TimeValueForm::Time, TIME)]
                .into_iter()
                .fold(CaseStatement::new(), |case, (form, term)| {
                    let iri = form.iri();
                    case.case(datatype().is_in([text_literal(term), text_literal(iri.as_str())]), text_literal(iri.as_str()))
                });
            (value().json_get(text_literal(keyword::VALUE)), declared.into())
        }
        Operand::Number(_) | Operand::Text(_) | Operand::Uri(_) | Operand::Boolean(_) => (value(), typed_null(PostgresType::Text)),
    }
}
