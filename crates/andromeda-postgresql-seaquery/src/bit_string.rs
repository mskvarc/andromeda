//! Fixed-width PostgreSQL bit strings (`bit(n)`) used as small sets.
//!
//! A set of at most `n` members, each owning one position, is one `bit(n)`
//! value: a member is in the set when its bit is 1. Positions are numbered
//! from the left, starting at 0, as `get_bit` numbers them. Two bit strings
//! combine only when their widths are equal, so every constructor here takes
//! the width and spells the explicit cast that pads a literal to it.

use sea_query::{BinOper, Expr, ExprTrait, Func, Iden, SimpleExpr};

/// The bitwise OR operator `|`, the union of two sets of one width.
const BIT_UNION: &str = "|";

/// The bit string functions.
#[derive(Iden, Clone, Copy)]
enum BitFunction {
    /// `get_bit`.
    GetBit,
    /// `bit_or`, the aggregate.
    BitOr,
}

/// The empty set of width `width`: `B'0'::bit(width)`, every bit 0.
///
/// An explicit cast pads a shorter bit string with zeros on the right, where
/// an assignment of one would be refused for its length.
#[must_use]
pub fn empty_bit_set(width: u16) -> SimpleExpr {
    Expr::cust(format!("B'0'::bit({width})"))
}

/// The set of width `width` holding the one member at `position`:
/// `B'1'::bit(width) >> position`.
#[must_use]
pub fn single_bit(width: u16, position: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs(format!("(B'1'::bit({width}) >> ($1))"), [position])
}

/// Whether the member at `position` is in `bits`: `get_bit(bits, position) = 1`.
#[must_use]
pub fn bit_is_set(bits: SimpleExpr, position: SimpleExpr) -> SimpleExpr {
    SimpleExpr::from(Func::cust(BitFunction::GetBit).args([bits, position])).eq(1)
}

/// The union of two sets of one width: `left | right`.
#[must_use]
pub fn bit_union(left: SimpleExpr, right: SimpleExpr) -> SimpleExpr {
    left.binary(BinOper::Custom(BIT_UNION), right)
}

/// The union of `bits` over the rows of a group: `bit_or(bits)`, NULL for an
/// empty group.
#[must_use]
pub fn bit_union_of_rows(bits: SimpleExpr) -> SimpleExpr {
    Func::cust(BitFunction::BitOr).args([bits]).into()
}

#[cfg(test)]
mod tests {
    use crate::bit_string::{bit_is_set, bit_union, bit_union_of_rows, empty_bit_set, single_bit};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    fn mask() -> SimpleExpr {
        Expr::col(Alias::new("mask"))
    }

    #[test]
    fn the_empty_set_is_zeros_padded_to_its_width() {
        assert_eq!(render(empty_bit_set(80)), "SELECT B'0'::bit(80)");
    }

    /// `get_bit` numbers positions from the left, so shifting a leading 1
    /// right by the position sets exactly that position.
    #[test]
    fn a_single_member_is_a_leading_one_shifted_to_its_position() {
        assert_eq!(render(single_bit(80, Expr::val(12))), "SELECT (B'1'::bit(80) >> (12))");
    }

    #[test]
    fn membership_reads_the_bit_at_the_position() {
        assert_eq!(render(bit_is_set(mask(), Expr::val(3))), r#"SELECT get_bit("mask", 3) = 1"#);
    }

    #[test]
    fn two_sets_unite_through_the_bitwise_or() {
        assert_eq!(render(bit_union(mask(), empty_bit_set(8))), r#"SELECT "mask" | (B'0'::bit(8))"#);
        assert_eq!(render(bit_union_of_rows(mask())), r#"SELECT bit_or("mask")"#);
    }
}
