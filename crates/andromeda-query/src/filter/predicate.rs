//! Leaf predicates of a `q` filter and the operators they carry (clause 4.9).

use crate::{
    filter::{path::AttrPath, scalar::CompareValue},
    pattern::Pattern,
};

/// A single filter predicate within a `q` expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    /// Attribute existence check (e.g. `temperature`).
    Exists(AttrPath),
    /// Comparison of an attribute value against a scalar, range, or value list.
    Compare {
        /// The attribute path to compare.
        path: AttrPath,
        /// The comparison operator.
        op: CmpOp,
        /// The right-hand side value(s).
        rhs: CompareValue,
    },
    /// Regex match: attribute value must match the pattern (e.g. `name~="A.*"`).
    MatchPattern {
        /// The attribute path to match against.
        path: AttrPath,
        /// The regex pattern, validated when the filter was parsed.
        pattern: Pattern,
    },
    /// Negated regex match: attribute value must not match the pattern (e.g. `name!~="A.*"`).
    NotMatchPattern {
        /// The attribute path to match against.
        path: AttrPath,
        /// The regex pattern that must not match, validated when the filter
        /// was parsed.
        pattern: Pattern,
    },
}

/// Comparison operators in `q` filter expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    /// Equal (`==`).
    Equal,
    /// Not equal (`!=`).
    Unequal,
    /// Greater than (`>`).
    Greater,
    /// Greater than or equal (`>=`).
    GreaterEq,
    /// Less than (`<`).
    Less,
    /// Less than or equal (`<=`).
    LessEq,
}

impl CmpOp {
    /// SQL operator this comparison renders to.
    ///
    /// Shared by every SQL rendering of clause 4.9, so a predicate cannot
    /// compile to one operator against `attribute_current`, another against the
    /// payload and a third against a registration's properties.
    pub const fn sql_operator(self) -> &'static str {
        match self {
            CmpOp::Equal => "=",
            CmpOp::Unequal => "!=",
            CmpOp::Greater => ">",
            CmpOp::GreaterEq => ">=",
            CmpOp::Less => "<",
            CmpOp::LessEq => "<=",
        }
    }

    /// Clause 4.9 token this comparison was written as.
    ///
    /// The inverse of what the parser consumes, used to serialise a filter back
    /// into query-string form. Distinct from [`CmpOp::sql_operator`] only at
    /// `==`, which SQL spells `=`.
    pub const fn ngsi_token(self) -> &'static str {
        match self {
            CmpOp::Equal => "==",
            CmpOp::Unequal => "!=",
            CmpOp::Greater => ">",
            CmpOp::GreaterEq => ">=",
            CmpOp::Less => "<",
            CmpOp::LessEq => "<=",
        }
    }
}

/// Whether a compound operand — a range or a value list — is asserted or denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetMembership {
    /// `==`: the attribute value lies inside the range or the list.
    Within,
    /// `!=`: it lies outside.
    Outside,
}

impl CmpOp {
    /// How this operator reads a compound operand, or `None` when clause 4.9
    /// admits none after it.
    ///
    /// Table 4.9-1 puts a range and a value list only after `==` and `!=`; the
    /// four ordering operators take a single scalar. Every rendering asks this
    /// rather than re-reading the clause, so an operator cannot become eligible
    /// against one backend and not another.
    pub const fn set_membership(self) -> Option<SetMembership> {
        match self {
            CmpOp::Equal => Some(SetMembership::Within),
            CmpOp::Unequal => Some(SetMembership::Outside),
            CmpOp::Greater | CmpOp::GreaterEq | CmpOp::Less | CmpOp::LessEq => None,
        }
    }
}

/// Whether a pattern predicate asserts a match or the absence of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternPolarity {
    /// `~=`
    Match,
    /// `!~=`
    NotMatch,
}

impl PatternPolarity {
    /// POSIX regex operator this polarity renders to.
    pub const fn operator(self) -> &'static str {
        match self {
            Self::Match => "~",
            Self::NotMatch => "!~",
        }
    }
}

/// Combine a presence test with a value test, as clause 4.9 requires.
///
/// Presence is conjoined in both polarities. Clause 4.9 states that the entity
/// "shall contain the target element" for a predicate to hold, and says so for
/// `notPatternOp` explicitly: `name!~="^A.*"` selects entities whose `name` is
/// present and does not match, not entities that have no `name` at all.
///
/// Every SQL rendering that tests a value builds its predicate here, which is
/// what stops one of them disjoining presence instead. One of them did: a
/// registration that simply lacked a property satisfied every negated `q`
/// predicate over it.
pub fn present_and(presence_sql: &str, value_sql: &str) -> String {
    format!("({presence_sql} AND {value_sql})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        csr::filter_sql::csr_expr_to_condition,
        filter::{
            eval::evaluate,
            parser,
            sql::{attribute_names::ResolvedAttributeNames, projection::compile, target::QFilterTarget},
        },
        sql::schema::Entities,
    };
    use andromeda_domain::temporal::AttributeNameId;
    use sea_query::{Condition, PostgresQueryBuilder, Query};

    #[test]
    fn only_equality_operators_admit_a_compound_operand() {
        // Clause 4.9 Table 4.9-1.
        assert_eq!(CmpOp::Equal.set_membership(), Some(SetMembership::Within));
        assert_eq!(CmpOp::Unequal.set_membership(), Some(SetMembership::Outside));
        for op in [CmpOp::Greater, CmpOp::GreaterEq, CmpOp::Less, CmpOp::LessEq] {
            assert_eq!(op.set_membership(), None, "{op:?}");
        }
    }

    #[test]
    fn a_value_test_is_always_conjoined_with_presence() {
        assert_eq!(present_and("x IS NOT NULL", "NOT (x ~ $1)"), "(x IS NOT NULL AND NOT (x ~ $1))");
    }

    // -- one reading of clause 4.9, three renderings ------------------------

    /// An attribute IRI used by the conformance fixtures.
    const SPEED: &str = "https://example.com/speed";
    /// An attribute IRI used by the conformance fixtures.
    const COLOR: &str = "https://example.com/color";
    /// An attribute IRI used by the conformance fixtures.
    const NAME: &str = "https://example.com/name";

    /// An entity carrying one value for each of the three fixture attributes.
    fn present(speed: i64, color: &str, name: &str) -> serde_json::Value {
        serde_json::json!([{
            "@id": "urn:ngsi-ld:Vehicle:A1",
            SPEED: [{ "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": speed}] }],
            COLOR: [{ "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": color}] }],
            NAME: [{ "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": name}] }],
        }])
    }

    /// An entity carrying none of the three fixture attributes.
    fn absent() -> serde_json::Value {
        serde_json::json!([{ "@id": "urn:ngsi-ld:Vehicle:A2" }])
    }

    fn names() -> ResolvedAttributeNames {
        ResolvedAttributeNames::from_pairs([
            (SPEED.parse().expect("valid attribute name"), AttributeNameId::from(1)),
            (COLOR.parse().expect("valid attribute name"), AttributeNameId::from(2)),
            (NAME.parse().expect("valid attribute name"), AttributeNameId::from(3)),
        ])
    }

    fn render(condition: Condition) -> String {
        Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .to_string(PostgresQueryBuilder)
    }

    /// One row of the clause 4.9 conformance table.
    struct Case {
        /// The filter, written against the three fixture attribute IRIs.
        expression: String,
        /// Entities the predicate must select.
        matching: Vec<serde_json::Value>,
        /// Entities carrying the attribute that the predicate must reject.
        non_matching: Vec<serde_json::Value>,
    }

    fn cases() -> Vec<Case> {
        let quoted = |iri: &str| format!("\"{iri}\"");
        vec![
            Case {
                expression: format!("{}>50", quoted(SPEED)),
                matching: vec![present(80, "red", "Alpha")],
                non_matching: vec![present(10, "red", "Alpha")],
            },
            Case {
                expression: format!("{}==10..20", quoted(SPEED)),
                matching: vec![present(15, "red", "Alpha")],
                non_matching: vec![present(25, "red", "Alpha")],
            },
            Case {
                expression: format!("{}!=10..20", quoted(SPEED)),
                matching: vec![present(25, "red", "Alpha")],
                non_matching: vec![present(15, "red", "Alpha")],
            },
            Case {
                expression: format!("{}==\"red\",\"blue\"", quoted(COLOR)),
                matching: vec![present(80, "red", "Alpha")],
                non_matching: vec![present(80, "green", "Alpha")],
            },
            Case {
                expression: format!("{}!=\"red\",\"blue\"", quoted(COLOR)),
                matching: vec![present(80, "green", "Alpha")],
                non_matching: vec![present(80, "red", "Alpha")],
            },
            Case {
                expression: format!("{}~=\"^A.*\"", quoted(NAME)),
                matching: vec![present(80, "red", "Alpha")],
                non_matching: vec![present(80, "red", "Beta")],
            },
            Case {
                expression: format!("{}!~=\"^A.*\"", quoted(NAME)),
                matching: vec![present(80, "red", "Beta")],
                non_matching: vec![present(80, "red", "Alpha")],
            },
            Case {
                expression: quoted(SPEED),
                matching: vec![present(80, "red", "Alpha")],
                non_matching: Vec::new(),
            },
        ]
    }

    /// Clause 4.9: an entity that does not carry the addressed element never
    /// satisfies a predicate over it, in either polarity. The in-memory
    /// evaluator decides that directly; the two SQL renderings decide it
    /// structurally, by conjoining the presence test rather than disjoining it,
    /// which is what these assertions pin.
    #[test]
    fn every_backend_reads_clause_four_nine_the_same_way() {
        for case in cases() {
            let expr = parser::parse(&case.expression).unwrap_or_else(|error| unreachable!("{}: {error}", case.expression));

            for entity in &case.matching {
                assert!(evaluate(&expr, entity), "{} must select {entity}", case.expression);
            }
            for entity in &case.non_matching {
                assert!(!evaluate(&expr, entity), "{} must reject {entity}", case.expression);
            }
            assert!(!evaluate(&expr, &absent()), "{} must reject an entity without the attribute", case.expression);

            let projection_sql = render(compile(&expr, QFilterTarget::Entities, &names()).unwrap_or_else(|error| unreachable!("{}: {error}", case.expression)));
            assert!(
                projection_sql.contains("EXISTS ("),
                "{} must test presence structurally: {projection_sql}",
                case.expression
            );
            assert!(
                !projection_sql.contains("IS NULL OR"),
                "{} must not admit a missing element: {projection_sql}",
                case.expression
            );

            let csr_sql = render(csr_expr_to_condition(&expr).unwrap_or_else(|error| unreachable!("{}: {error}", case.expression)));
            assert!(
                !csr_sql.contains("IS NULL OR"),
                "{} must not admit a registration without the property: {csr_sql}",
                case.expression
            );
        }
    }
}
