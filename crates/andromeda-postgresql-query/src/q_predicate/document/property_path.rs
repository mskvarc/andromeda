//! Where a Query Term's target lives in a document of Properties: the node of
//! the last (sub-)Property its path names, the value that node holds, and the
//! compound member a trailing group steps to.

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
    compact::classes::{DATE, DATE_TIME, TIME},
    keyword,
};
use andromeda_postgresql_seaquery::{
    cast::typed_null,
    column_reference::ColumnReference,
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
    /// A member the node carries under the member's expanded IRI, read as one
    /// more sub-property: a document holds a property's members as its writer
    /// keyed them.
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
    /// `LanguageProperty` holds. A term without a target satisfies no
    /// document, though it is valid `q` syntax.
    #[must_use]
    pub fn of(term: &'a TermReading) -> Option<PropertyPath<'a>> {
        if !term.hops().is_empty() {
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
    /// it is absent: the node the path ends on, or the compound member its
    /// trailing group steps to.
    ///
    /// Clause 4.9 makes an existence term select what defines the target
    /// element "regardless of any target value", so a node with no value is
    /// present, and a member is present when the value holds it.
    #[must_use]
    pub fn element(self, document: &ColumnReference) -> SimpleExpr {
        match self.ending {
            Ending::Node | Ending::InstanceMember(_) => self.node(document),
            Ending::Members(_) => self.value(document),
        }
    }

    /// The target value the path names in `document`, NULL when it is absent:
    /// the value its last node holds, unwrapped once, and each compound member
    /// stepped into that value.
    ///
    /// A compound member is an ordinary JSON key of the unwrapped value, not a
    /// `Property` node of its own, so it is stepped without unwrapping: its
    /// value carries no `type` member a wrapper would test.
    #[must_use]
    pub fn value(self, document: &ColumnReference) -> SimpleExpr {
        let members = match self.ending {
            Ending::Members(members) => members,
            Ending::Node | Ending::InstanceMember(_) => &[],
        };
        members.iter().fold(unwrap_property(self.node(document)), |value, member| {
            value.json_get(text_literal(member.as_str()))
        })
    }

    /// The node of the last (sub-)Property the path names in `document`.
    ///
    /// Clause 4.9 makes each name after the first of a path "a (sub)Property
    /// or (sub)Relationship", and a sub-property is a member of its property's
    /// node, beside the node's value.
    fn node(self, document: &ColumnReference) -> SimpleExpr {
        let node = self
            .sub_properties
            .iter()
            .fold(document.column_expr().json_get(text_literal(self.attribute.as_str())), |node, sub| {
                node.json_get(text_literal(sub.as_str()))
            });
        match self.ending {
            Ending::InstanceMember(member) => node.json_get(text_literal(member.iri().as_str())),
            Ending::Node | Ending::Members(_) => node,
        }
    }
}

/// Unwrap one normalized `Property` node, reading `node` exactly once.
///
/// The node is aliased as `j` in a scalar subquery, so the `CASE` reads a
/// column three times rather than embedding the node expression three times.
fn unwrap_property(node: SimpleExpr) -> SimpleExpr {
    let derived = Query::select().expr_as(node, Alias::new("j")).to_owned();
    let j = || SimpleExpr::from(SeaExpr::col(Alias::new("j")));
    let case = CaseStatement::new()
        .case(
            SeaExpr::expr(j().json_get_text(text_literal("type"))).eq(text_literal("Property")),
            j().json_get(text_literal("value")),
        )
        .finally(j());
    let unwrapped = Query::select().expr(case).from_subquery(derived, Alias::new("s")).to_owned();
    SimpleExpr::SubQuery(None, Box::new(unwrapped.into_sub_query_statement()))
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
#[must_use]
pub fn declared_reading(value: impl Fn() -> SimpleExpr, operand: &Operand) -> (SimpleExpr, SimpleExpr) {
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
