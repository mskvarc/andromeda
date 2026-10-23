//! The combinations of attribute family and value shape a stream holds, as
//! the one `bit(80)` value its catalogue row carries.
//!
//! Clause 5.7.4.4 refuses an aggregation over an attribute that is not
//! eligible for a requested method, and Tables 4.5.19.1-1 to 4.5.19.1-3
//! decide eligibility from both the family an instance was written as (it
//! selects the table) and the refined shape its value was recorded in (it
//! selects the column). A stream's mask holds one bit per combination its
//! instances carry, tombstones excepted: a deletion carries no value.
//!
//! Every family and every shape owns a frozen code counted from 1, so the
//! combination of family code `k` and shape code `s` owns position
//! `(k - 1) * 10 + (s - 1)`, and the width is the product of the two
//! registries' sizes. The positions are a stored contract for the same reason
//! the codes are.

use crate::{
    alias::Ah,
    recorded_shape::{RecordedShapeColumn, recorded_shape},
};
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, recorded_shape::RecordedShape};
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{
    bit_string::{bit_is_set, empty_bit_set, single_bit},
    cast::cast,
    catalog_function::generate_series,
    literal::int_literal,
    pg_type::PostgresType,
};
use sea_query::{Expr, ExprTrait, Iden, Query, SelectStatement, SimpleExpr};

/// The width of a mask: one bit for each of the 8 families times each of the
/// 10 shapes.
pub const SHAPE_MASK_WIDTH: u16 = 80;

/// How many positions one family spans: one per shape.
const SHAPES_PER_FAMILY: i64 = 10;

/// Alias `position`: one position of a mask, as [`held_combinations`] walks
/// them.
#[derive(Iden, Clone, Copy)]
#[iden = "position"]
struct Position;

/// The position the combination of `kind` and `shape` owns.
#[must_use]
pub fn combination_position(kind: AttributeKind, shape: RecordedShape) -> i64 {
    (i64::from(kind.code()) - 1) * SHAPES_PER_FAMILY + (i64::from(shape.code()) - 1)
}

/// The mask holding no combination, which a stream carries until it records a
/// value.
#[must_use]
pub fn no_shapes() -> SimpleExpr {
    empty_bit_set(SHAPE_MASK_WIDTH)
}

/// Whether `mask` holds the combination of `kind` and `shape`.
#[must_use]
pub fn holds_combination(mask: SimpleExpr, kind: AttributeKind, shape: RecordedShape) -> SimpleExpr {
    bit_is_set(mask, int_literal(combination_position(kind, shape)))
}

/// The mask holding the one combination the `attribute_history` row aliased
/// `ah` carries: its family and the refined shape [`recorded_shape`] reads.
///
/// A tombstone has no shape, so its mask is SQL NULL, which `bit_or` skips.
#[must_use]
pub fn recorded_combination() -> SimpleExpr {
    let family = SimpleExpr::from(Expr::col((Ah, AttributeHistory::AttributeKind))).sub(1);
    let shape = recorded_shape().sub(1);
    single_bit(SHAPE_MASK_WIDTH, family.mul(int_literal(SHAPES_PER_FAMILY)).add(shape))
}

/// `FROM generate_series(0, 79) AS position WHERE get_bit(mask, position) =
/// 1`: one row per position `mask` holds, projecting nothing yet.
#[must_use]
pub fn held_positions(mask: SimpleExpr) -> SelectStatement {
    Query::select()
        .from_function(generate_series(int_literal(0), int_literal(i64::from(SHAPE_MASK_WIDTH) - 1)), Position)
        .and_where(bit_is_set(mask, position()))
        .take()
}

/// The mask holding the one position [`held_positions`] walks.
#[must_use]
pub fn position_mask() -> SimpleExpr {
    single_bit(SHAPE_MASK_WIDTH, position())
}

/// One row per combination `mask` holds, its family code projected as
/// `attribute_kind` and its shape code as [`RecordedShapeColumn`], both
/// `smallint` like the history columns they describe.
#[must_use]
pub fn held_combinations(mask: SimpleExpr) -> SelectStatement {
    let code = |code: SimpleExpr| cast(code.add(1), PostgresType::Smallint);
    held_positions(mask)
        .expr_as(code(position().div(int_literal(SHAPES_PER_FAMILY))), AttributeHistory::AttributeKind)
        .expr_as(code(position().modulo(int_literal(SHAPES_PER_FAMILY))), RecordedShapeColumn)
        .take()
}

/// The position [`held_positions`] walks.
fn position() -> SimpleExpr {
    Expr::col(Position)
}

#[cfg(test)]
mod tests {
    use crate::shape_mask::{SHAPE_MASK_WIDTH, SHAPES_PER_FAMILY, combination_position, held_combinations, holds_combination, recorded_combination};
    use andromeda_postgresql_codes::{attribute_kind::AttributeKind, recorded_shape::RecordedShape};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};
    use std::collections::HashSet;

    /// The positions are computed from the codes, so the codes have to count
    /// from 1 without a gap for every combination to own a position of its
    /// own inside the width.
    #[test]
    fn every_combination_owns_its_own_position_inside_the_width() {
        assert_eq!(usize::from(SHAPE_MASK_WIDTH), AttributeKind::ALL.len() * RecordedShape::ALL.len());
        assert_eq!(usize::try_from(SHAPES_PER_FAMILY).ok(), Some(RecordedShape::ALL.len()));
        let positions: HashSet<i64> = AttributeKind::ALL
            .iter()
            .flat_map(|kind| RecordedShape::ALL.iter().map(|shape| combination_position(*kind, *shape)))
            .collect();
        assert_eq!(positions.len(), usize::from(SHAPE_MASK_WIDTH));
        assert!(positions.iter().all(|position| (0..i64::from(SHAPE_MASK_WIDTH)).contains(position)));
    }

    #[test]
    fn a_family_spans_one_run_of_positions_in_shape_order() {
        assert_eq!(combination_position(AttributeKind::Property, RecordedShape::Number), 0);
        assert_eq!(combination_position(AttributeKind::Property, RecordedShape::Uri), 9);
        assert_eq!(combination_position(AttributeKind::Relationship, RecordedShape::Uri), 19);
        assert_eq!(combination_position(AttributeKind::ListRelationship, RecordedShape::Uri), 79);
    }

    #[test]
    fn membership_reads_the_bit_of_the_combination() {
        let sql = Query::select()
            .expr(holds_combination(
                Expr::col(Alias::new("shape_mask")),
                AttributeKind::Property,
                RecordedShape::String,
            ))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT get_bit("shape_mask", 1) = 1"#);
    }

    /// The bit is read off the same refined shape an aggregation projects, so
    /// the mask names exactly the shapes the rows carry.
    #[test]
    fn a_recorded_row_sets_the_bit_of_its_family_and_refined_shape() {
        let sql = Query::select().expr(recorded_combination()).to_owned().to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"SELECT (B'1'::bit(80) >> ((("ah"."attribute_kind" - 1) * (10)) + ((CASE"#),
            "{sql}"
        );
        assert!(sql.ends_with("END) - 1)))"), "{sql}");
    }

    #[test]
    fn a_mask_decodes_to_one_row_per_held_combination() {
        let sql = held_combinations(Expr::col(Alias::new("shape_mask"))).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT (("position" / (10)) + 1)::smallint AS "attribute_kind", (("position" % (10)) + 1)::smallint AS "recorded_shape""#,
                r#" FROM generate_series(0, 79) AS "position" WHERE get_bit("shape_mask", "position") = 1"#
            )
        );
    }
}
