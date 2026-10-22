//! Typed PostgreSQL casts.
//!
//! Every `expr::type` a statement needs is built here from a [`PostgresType`],
//! so the type token is named once and no cast is spelled as a scattered
//! `"x"::numeric` string. The postfix `::` spelling is kept rather than
//! `CAST(x AS type)` because it is the form every rendered-SQL test in the store
//! and query crates already pins, so a converted statement's cast stays
//! byte-for-byte what it was.
//!
//! `::` binds tighter than every arithmetic and comparison operator, so an
//! operand built from an infix or prefix operator is parenthesised before the
//! cast; an operand spelled as custom text carries an operator the builder
//! cannot see, so it goes through [`cast_compound`].

use crate::pg_type::PostgresType;
use sea_query::{Expr, SimpleExpr};

/// Cast `expr` to `ty`, rendered `expr::ty`, or `(expr)::ty` when the
/// operand is an operator expression the cast would otherwise bind into.
#[must_use]
pub fn cast(expr: SimpleExpr, ty: PostgresType) -> SimpleExpr {
    // `::` binds tighter than every operator, so an operator expression is
    // parenthesised. The crate is the sanctioned home for a hand-spelled
    // fragment; the type token still comes from the iden so the cast and the
    // column that carries the type name it identically.
    let operand = match &expr {
        SimpleExpr::Binary(..) | SimpleExpr::Unary(..) => "($1)",
        SimpleExpr::Column(_)
        | SimpleExpr::Tuple(_)
        | SimpleExpr::FunctionCall(_)
        | SimpleExpr::SubQuery(..)
        | SimpleExpr::Value(_)
        | SimpleExpr::Values(_)
        | SimpleExpr::Custom(_)
        | SimpleExpr::CustomWithExpr(..)
        | SimpleExpr::Keyword(_)
        | SimpleExpr::AsEnum(..)
        | SimpleExpr::Case(_)
        | SimpleExpr::Constant(_)
        | SimpleExpr::TypeName(_) => "$1",
        // sea-query's expression enum is non-exhaustive; parentheses are correct
        // around any operand, so a variant added upstream is cast as compound.
        _unforeseen => "($1)",
    };
    Expr::cust_with_exprs(format!("{operand}::{ty}", ty = type_token(ty)), [expr])
}

/// Cast a compound expression to `ty`, rendered `(expr)::ty`.
///
/// `::` binds tighter than every arithmetic and comparison operator, so a
/// compound operand (anything with its own infix operator, such as a JSONB
/// `#>>` extraction) must be parenthesised or the cast would bind to only its
/// right-hand side. [`cast`] does so for the operators the builder models; this
/// is for an operand whose operator is spelled as custom text.
#[must_use]
pub fn cast_compound(expr: SimpleExpr, ty: PostgresType) -> SimpleExpr {
    Expr::cust_with_exprs(format!("($1)::{ty}", ty = type_token(ty)), [expr])
}

/// A typed SQL `NULL`, rendered `NULL::ty` inline.
///
/// A column a statement must always project but that carries no value on a given
/// row - the aggregation `bucket_start`/`bucket_end` over a whole-range group -
/// is a typed null so the result column keeps its declared type rather than
/// arriving as an untyped `NULL` the driver cannot decode.
#[must_use]
pub fn typed_null(ty: PostgresType) -> SimpleExpr {
    Expr::cust(format!("NULL::{ty}", ty = type_token(ty)))
}

/// The bare type token an iden renders, without the double-quoting a column
/// reference gets: a type name is not an identifier a cast quotes.
fn type_token(ty: PostgresType) -> String {
    sea_query::Iden::to_string(&ty)
}

#[cfg(test)]
mod tests {
    use crate::{cast::cast, pg_type::PostgresType};
    use sea_query::{Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_column_casts_with_the_postfix_operator() {
        let sql = render(cast(Expr::col(sea_query::Alias::new("value_number")), PostgresType::Numeric));
        assert!(sql.contains("::numeric"), "{sql}");
        assert!(!sql.contains("CAST("), "the postfix form is kept, not CAST(): {sql}");
    }

    /// A number reaches the arbitrary-precision column as decimal text cast in
    /// the statement, never widened to binary floating point.
    #[test]
    fn a_numeric_cast_never_introduces_a_float() {
        let sql = render(cast(Expr::val("12345678901234567890"), PostgresType::Numeric));
        assert!(sql.contains("::numeric"), "{sql}");
        assert!(!sql.contains("double precision"), "{sql}");
        assert!(!sql.contains("float8"), "{sql}");
    }

    #[test]
    fn a_typed_null_carries_its_type() {
        use crate::cast::typed_null;
        let sql = render(typed_null(PostgresType::Timestamptz));
        assert!(sql.contains("NULL::timestamptz"), "{sql}");
    }

    #[test]
    fn a_compound_operand_is_parenthesised_before_the_cast() {
        use crate::cast::cast_compound;
        let extraction = Expr::cust(r#""inst"."value" #>> $1"#);
        let sql = render(cast_compound(extraction, PostgresType::Timestamptz));
        assert!(sql.contains(r#"("inst"."value" #>> $1)::timestamptz"#), "{sql}");
    }

    #[test]
    fn an_operator_operand_is_parenthesised_before_the_cast() {
        use sea_query::ExprTrait;
        let sql = render(cast(Expr::col(sea_query::Alias::new("g")).modulo(5000), PostgresType::Text));
        assert!(sql.contains(r#"("g" % 5000)::text"#), "{sql}");
    }

    #[test]
    fn each_type_renders_its_own_token() {
        assert!(render(cast(Expr::val(0), PostgresType::Boolean)).contains("::boolean"));
        assert!(render(cast(Expr::val(0), PostgresType::Timestamptz)).contains("::timestamptz"));
        assert!(render(cast(Expr::val(0), PostgresType::Geography)).contains("::geography"));
        assert!(render(cast(Expr::val(0), PostgresType::Interval)).contains("::interval"));
        assert!(render(cast(Expr::val(0), PostgresType::IntegerArray)).contains("::integer[]"));
        assert!(render(cast(Expr::val(0), PostgresType::TextArray)).contains("::text[]"));
        assert!(render(cast(Expr::val(0), PostgresType::Lquery)).contains("::lquery"));
    }
}
