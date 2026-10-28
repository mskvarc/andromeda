//! The bounds a check expression puts on its columns.
//!
//! A check bounds a column when it compares the column, or the column's byte
//! length, with a constant: `col >= 0`, `col between 1 and 9`, `col in (1,
//! 2)`, `col = any (array[1, 2])`, `col <@ array[1, 2]`, `octet_length(col) <=
//! 1024`, and for an array `1 <= all (col)`, which bounds every element.
//! Comparisons are read through `and`, `or` and `not`, so a bound written as
//! `col is null or col >= 0` counts.

use pg_query::{
    Node,
    NodeEnum,
    protobuf::{AExpr, AExprKind, a_const::Val},
};

/// The function a byte-length bound applies to its column.
const OCTET_LENGTH: &str = "octet_length";

/// What a check states about a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// The column is compared with a constant: `col <op> value`, normalised so
    /// the column is on the left.
    Compared(Comparison, i64),
    /// The column lies in a closed range of constants.
    Between(i64, i64),
    /// The column is one of a list of values.
    OneOf,
    /// The column's byte length is compared with a constant, normalised so
    /// the length is on the left.
    ByteLength(Comparison, i64),
}

/// A comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    /// `<`.
    Less,
    /// `<=`.
    LessOrEqual,
    /// `=`.
    Equal,
    /// `>=`.
    GreaterOrEqual,
    /// `>`.
    Greater,
}

impl Comparison {
    /// The operator `name` spells.
    fn of(name: &str) -> Option<Comparison> {
        match name {
            "<" => Some(Comparison::Less),
            "<=" => Some(Comparison::LessOrEqual),
            "=" => Some(Comparison::Equal),
            ">=" => Some(Comparison::GreaterOrEqual),
            ">" => Some(Comparison::Greater),
            _ => None,
        }
    }

    /// The same comparison with its operands swapped.
    const fn flipped(self) -> Comparison {
        match self {
            Comparison::Less => Comparison::Greater,
            Comparison::LessOrEqual => Comparison::GreaterOrEqual,
            Comparison::Equal => Comparison::Equal,
            Comparison::GreaterOrEqual => Comparison::LessOrEqual,
            Comparison::Greater => Comparison::Less,
        }
    }
}

/// One bound a check states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnBound {
    /// The column bounded.
    column: String,
    /// The bound.
    bound: Bound,
}

impl ColumnBound {
    /// The column bounded.
    #[must_use]
    pub fn column(&self) -> &str {
        &self.column
    }

    /// The bound.
    #[must_use]
    pub const fn bound(&self) -> Bound {
        self.bound
    }

    /// Whether the bound keeps the column at zero or above.
    #[must_use]
    pub const fn is_non_negative(&self) -> bool {
        match self.bound {
            Bound::Compared(Comparison::GreaterOrEqual | Comparison::Equal, value) | Bound::Between(value, _) => value >= 0,
            Bound::Compared(Comparison::Greater, value) => value >= -1,
            Bound::Compared(Comparison::Less | Comparison::LessOrEqual, _) | Bound::OneOf | Bound::ByteLength(..) => false,
        }
    }

    /// The largest byte length the bound admits, for a byte-length bound from
    /// above.
    #[must_use]
    pub const fn byte_length_limit(&self) -> Option<i64> {
        match self.bound {
            Bound::ByteLength(Comparison::LessOrEqual, value) => Some(value),
            Bound::ByteLength(Comparison::Less, value) => Some(value - 1),
            Bound::ByteLength(Comparison::Equal | Comparison::GreaterOrEqual | Comparison::Greater, _)
            | Bound::Compared(..)
            | Bound::Between(..)
            | Bound::OneOf => None,
        }
    }
}

/// Every bound `expression` states.
#[must_use]
pub fn bounds_of(expression: &Node) -> Vec<ColumnBound> {
    let mut bounds = Vec::new();
    collect(expression, &mut bounds);
    bounds
}

/// Gather the bounds of `expression` into `bounds`.
fn collect(expression: &Node, bounds: &mut Vec<ColumnBound>) {
    match &expression.node {
        Some(NodeEnum::BoolExpr(combined)) => {
            for argument in &combined.args {
                collect(argument, bounds);
            }
        }
        Some(NodeEnum::AExpr(comparison)) => bounds.extend(bound_of(comparison)),
        Some(_) | None => {}
    }
}

/// The bound one operator expression states, if it states one.
fn bound_of(expression: &AExpr) -> Option<ColumnBound> {
    let left = expression.lexpr.as_deref()?;
    let right = expression.rexpr.as_deref()?;
    let operator = expression.name.iter().find_map(|name| match &name.node {
        Some(NodeEnum::String(name)) => Some(name.sval.as_str()),
        Some(_) | None => None,
    })?;
    match AExprKind::try_from(expression.kind).ok()? {
        AExprKind::AexprOp if operator == "<@" => Some(ColumnBound {
            column: column_of(left)?.to_owned(),
            bound: Bound::OneOf,
        }),
        AExprKind::AexprOp | AExprKind::AexprOpAll => {
            let comparison = Comparison::of(operator)?;
            let (operand, comparison, value) = match (integer_of(left), integer_of(right)) {
                (None, Some(value)) => (left, comparison, value),
                (Some(value), None) => (right, comparison.flipped(), value),
                (Some(_), Some(_)) | (None, None) => return None,
            };
            if let Some(column) = column_of(operand) {
                return Some(ColumnBound {
                    column: column.to_owned(),
                    bound: Bound::Compared(comparison, value),
                });
            }
            Some(ColumnBound {
                column: byte_length_column(operand)?.to_owned(),
                bound: Bound::ByteLength(comparison, value),
            })
        }
        AExprKind::AexprIn | AExprKind::AexprOpAny => Some(ColumnBound {
            column: column_of(left)?.to_owned(),
            bound: Bound::OneOf,
        }),
        AExprKind::AexprBetween => {
            let Some(NodeEnum::List(range)) = &right.node else {
                return None;
            };
            let [low, high] = range.items.as_slice() else {
                return None;
            };
            Some(ColumnBound {
                column: column_of(left)?.to_owned(),
                bound: Bound::Between(integer_of(low)?, integer_of(high)?),
            })
        }
        AExprKind::Undefined
        | AExprKind::AexprDistinct
        | AExprKind::AexprNotDistinct
        | AExprKind::AexprNullif
        | AExprKind::AexprLike
        | AExprKind::AexprIlike
        | AExprKind::AexprSimilar
        | AExprKind::AexprNotBetween
        | AExprKind::AexprBetweenSym
        | AExprKind::AexprNotBetweenSym => None,
    }
}

/// The column `node` references, unqualified or by its last name part.
fn column_of(node: &Node) -> Option<&str> {
    match &node.node {
        Some(NodeEnum::ColumnRef(column)) => column.fields.last().and_then(|field| match &field.node {
            Some(NodeEnum::String(name)) => Some(name.sval.as_str()),
            Some(_) | None => None,
        }),
        Some(NodeEnum::TypeCast(cast)) => cast.arg.as_deref().and_then(column_of),
        Some(_) | None => None,
    }
}

/// The column `node` measures with `octet_length`.
fn byte_length_column(node: &Node) -> Option<&str> {
    let Some(NodeEnum::FuncCall(call)) = &node.node else {
        return None;
    };
    let is_octet_length = call
        .funcname
        .last()
        .is_some_and(|name| matches!(&name.node, Some(NodeEnum::String(name)) if name.sval == OCTET_LENGTH));
    match call.args.as_slice() {
        [argument] if is_octet_length => column_of(argument),
        _ => None,
    }
}

/// The integer constant `node` is, a cast or a minus sign around it included.
fn integer_of(node: &Node) -> Option<i64> {
    match &node.node {
        Some(NodeEnum::AConst(constant)) => match &constant.val {
            Some(Val::Ival(integer)) => Some(i64::from(integer.ival)),
            Some(Val::Fval(float)) => float.fval.parse().ok(),
            Some(Val::Boolval(_) | Val::Sval(_) | Val::Bsval(_)) | None => None,
        },
        Some(NodeEnum::TypeCast(cast)) => cast.arg.as_deref().and_then(integer_of),
        Some(NodeEnum::AExpr(negation)) if negation.lexpr.is_none() => negation.rexpr.as_deref().and_then(integer_of).map(|value| -value),
        Some(_) | None => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::declared_schema::check_bound::{Bound, ColumnBound, Comparison, bounds_of};
    use pretty_assertions::assert_eq;

    /// The bounds of the check `expression`.
    fn bounds(expression: &str) -> Vec<(String, Bound)> {
        let parsed = pg_query::parse(&format!("select {expression}"), 0).ok();
        let Some(pg_query::NodeEnum::SelectStmt(select)) = parsed
            .and_then(|parsed| parsed.protobuf.stmts.into_iter().next())
            .and_then(|statement| statement.stmt)
            .and_then(|statement| statement.node)
        else {
            return Vec::new();
        };
        let Some(pg_query::NodeEnum::ResTarget(target)) = select.target_list.into_iter().next().and_then(|target| target.node) else {
            return Vec::new();
        };
        target
            .val
            .map(|value| bounds_of(&value))
            .unwrap_or_default()
            .into_iter()
            .map(|bound: ColumnBound| (bound.column().to_owned(), bound.bound()))
            .collect()
    }

    #[test]
    fn a_comparison_is_read_with_the_column_on_the_left() {
        assert_eq!(
            bounds("0 <= attempt_count"),
            [("attempt_count".to_owned(), Bound::Compared(Comparison::GreaterOrEqual, 0))]
        );
    }

    #[test]
    fn bounds_are_read_through_or_and_not_null_tests() {
        assert_eq!(
            bounds("last_bytes is null or (last_bytes >= 0 and kind between 1 and 3)"),
            [
                ("last_bytes".to_owned(), Bound::Compared(Comparison::GreaterOrEqual, 0)),
                ("kind".to_owned(), Bound::Between(1, 3)),
            ]
        );
    }

    #[test]
    fn a_byte_length_bound_and_a_value_list_are_read() {
        assert_eq!(
            bounds("octet_length(id) <= 1024 and kinds <@ array[1, 2]::smallint[] and code in (1, 2) and 48 >= all (operations)"),
            [
                ("id".to_owned(), Bound::ByteLength(Comparison::LessOrEqual, 1024)),
                ("kinds".to_owned(), Bound::OneOf),
                ("code".to_owned(), Bound::OneOf),
                ("operations".to_owned(), Bound::Compared(Comparison::LessOrEqual, 48)),
            ]
        );
    }
}
