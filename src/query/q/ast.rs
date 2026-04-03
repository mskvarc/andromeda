//! Abstract syntax tree for NGSI-LD `q` filter expressions.

use serde_json::Number;

/// A parsed `q` filter expression tree.
///
/// Represents the boolean structure of an NGSI-LD query filter:
/// `And` and `Or` nodes combine sub-expressions; `Pred` leaves contain
/// individual predicates (existence checks, comparisons, pattern matches).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// Conjunction: all sub-expressions must be true (`;` in NGSI-LD syntax).
    And(Vec<Expr>),
    /// Disjunction: at least one sub-expression must be true (`|` in NGSI-LD syntax).
    Or(Vec<Expr>),
    /// A leaf predicate.
    Pred(Predicate),
}

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
        /// The regex pattern.
        pattern: String,
    },
    /// Negated regex match: attribute value must not match the pattern (e.g. `name!~="A.*"`).
    NotMatchPattern {
        /// The attribute path to match against.
        path: AttrPath,
        /// The regex pattern that must not match.
        pattern: String,
    },
}

/// An attribute path within a `q` expression, potentially including sub-attributes
/// and compound value navigation.
///
/// Example: `address[city].streetName` would have `attr = "address"`,
/// `sub_attrs = ["city"]`, `value_path = ["streetName"]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrPath {
    /// The top-level attribute name (compact or expanded IRI).
    pub attr: String,
    /// Sub-attribute names for navigating nested attributes.
    pub sub_attrs: Vec<String>,
    /// Keys for navigating into compound values (JSON object paths).
    pub value_path: Vec<String>,
}

#[cfg(test)]
impl AttrPath {
    pub fn simple(attr: impl Into<String>) -> AttrPath {
        AttrPath {
            attr: attr.into(),
            sub_attrs: Vec::new(),
            value_path: Vec::new(),
        }
    }
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

/// The right-hand side of a comparison predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareValue {
    /// A single scalar value (e.g. `speed>50`).
    Scalar(Scalar),
    /// A range of two values (e.g. `speed==10..20`).
    Range {
        /// The lower bound of the range (inclusive).
        min: Scalar,
        /// The upper bound of the range (inclusive).
        max: Scalar,
    },
    /// A list of values (e.g. `status==(\"active\",\"pending\")`).
    ValueList(Vec<Scalar>),
}

/// A scalar value in a `q` expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scalar {
    /// A numeric value (integer or floating-point).
    Number(Number),
    /// A boolean value (`true` or `false`).
    Bool(bool),
    /// A quoted string value.
    Str(String),
    /// An ISO 8601 date-time value.
    DateTime(String),
    /// An ISO 8601 date value.
    Date(String),
    /// An ISO 8601 time value.
    Time(String),
}
