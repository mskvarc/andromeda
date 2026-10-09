//! The `UNNEST(array, …) WITH ORDINALITY` set-write table source.
//!
//! A set-based write turns one parallel array per column into a table and
//! inserts every row of it in a single statement, in batch order. sea-query's
//! [`sea_query::TableRef`] is a closed enum with no `WITH ORDINALITY` variant
//! and no column-list alias, and [`sea_query::SelectStatement::from_function`]
//! force-quotes the alias, so this source cannot be a native table reference.
//!
//! The split this module makes keeps every bound value inside sea-query anyway:
//! [`unnest_call`] builds the `UNNEST(…)` function call with each array cast to
//! its element type, which `from_function` binds and numbers exactly like any
//! other statement value. Only the `WITH ORDINALITY AS d(col, …)` suffix — which
//! carries no bound value, just identifiers — is applied to the rendered SQL by
//! [`apply_with_ordinality`], here in the one crate sanctioned to spell dialect
//! SQL. The set-write therefore binds through `build_sqlx` like every other
//! converted statement, with no hand-numbered parameters.

use crate::{
    cast::cast,
    column_reference::{qualified, quoted},
    pg_type::PostgresType,
};
use sea_query::{Alias, Condition, DynIden, Expr, Func, FunctionCall, Iden, IntoIden, SimpleExpr};

/// The `UNNEST` set-returning function.
#[derive(Iden, Clone, Copy)]
#[iden = "UNNEST"]
struct UnnestFunction;

/// One column an `UNNEST` set-write projects: the name the row column takes in
/// the `AS d(…)` alias, and the array element type its argument is cast to.
///
/// The element type is the array form (for example [`PostgresType::BigintArray`]),
/// so the argument renders `$n::bigint[]` — the shape `UNNEST` expects and the
/// element type the projected column carries.
#[derive(Clone)]
pub struct UnnestColumn {
    /// Column name in the table-source alias.
    name: DynIden,
    /// Array type the `UNNEST` argument is cast to.
    array_type: PostgresType,
}

impl UnnestColumn {
    /// A column named `name`, whose array argument casts to `array_type`.
    pub fn new(name: impl IntoIden, array_type: PostgresType) -> UnnestColumn {
        UnnestColumn {
            name: name.into_iden(),
            array_type,
        }
    }
}

/// `UNNEST(array)` over a single array, as a table source or a projection.
///
/// The parallel-array set-write uses [`unnest_call`]; this is the single-array
/// form a type or scope discovery expands one column with, returned as a
/// [`FunctionCall`] so it stands either in a `FROM` position (`unnest(types) AS
/// t`) or, converted to an expression, in a projection (`unnest(types)`).
#[must_use]
pub fn unnest_of(array: SimpleExpr) -> FunctionCall {
    Func::cust(UnnestFunction).args([array])
}

/// Build the `UNNEST($n::t[], …)` call, one cast argument per column.
///
/// Each item pairs a column with its array expression, so the argument order
/// matches the column order the alias later names and the cast type is the
/// column's own. sea-query binds the array expressions and numbers the
/// placeholders, so no parameter is hand-numbered.
pub fn unnest_call<'column>(arrays: impl IntoIterator<Item = (&'column UnnestColumn, SimpleExpr)>) -> FunctionCall {
    let arguments = arrays.into_iter().map(|(column, array)| cast(array, column.array_type));
    Func::cust(UnnestFunction).args(arguments)
}

/// Rewrite the `AS "alias"` that `from_function` emits for the `UNNEST` source
/// into `WITH ORDINALITY AS alias(col, …, ordinality)`.
///
/// The alias must be one sea-query rendered nowhere else in the statement — a
/// set-write reads its columns as `alias.column`, never as `AS "alias"`, so the
/// single `AS "alias"` the table source produces is the only match. The suffix
/// holds identifiers only, so no bound value moves and the statement's parameter
/// numbering is untouched.
pub fn apply_with_ordinality(mut sql: String, alias: &dyn Iden, columns: &[UnnestColumn], ordinality: &dyn Iden) -> String {
    let emitted = format!("AS {}", quoted(alias));
    let mut names: Vec<String> = columns.iter().map(|column| column.name.to_string()).collect();
    names.push(Iden::to_string(ordinality));
    let names = names;
    let replacement = format!("WITH ORDINALITY AS {}({})", Iden::to_string(alias), names.join(", "));
    if let Some(position) = sql.find(&emitted) {
        sql.replace_range(position..position + emitted.len(), &replacement);
    }
    sql
}

/// Rewrite the `AS "alias"` that `from_function` emits for the `UNNEST` source
/// into `AS alias(col, …)` — a column-list alias with no ordinality.
///
/// This is the plain parallel-array set-write: each `UNNEST` argument becomes a
/// named row column, and no row position is projected. Like
/// [`apply_with_ordinality`], the suffix holds identifiers only, so no bound
/// value moves and the statement's parameter numbering is untouched. The alias
/// must be one sea-query renders nowhere else, so its single `AS "alias"` is the
/// only match.
pub fn apply_column_alias(mut sql: String, alias: &dyn Iden, columns: &[UnnestColumn]) -> String {
    let emitted = format!("AS {}", quoted(alias));
    let names: Vec<String> = columns.iter().map(|column| column.name.to_string()).collect();
    let replacement = format!("AS {}({})", Iden::to_string(alias), names.join(", "));
    if let Some(position) = sql.find(&emitted) {
        sql.replace_range(position..position + emitted.len(), &replacement);
    }
    sql
}

/// The column `WITH ORDINALITY` adds to an unnested array: the 1-based
/// position of each element.
#[derive(Iden, Clone, Copy)]
#[iden = "ordinality"]
pub struct Ordinality;

/// Whether some path of elements through a tree held as one array passes
/// `filter`: `EXISTS (SELECT 1 FROM UNNEST(tree) WITH ORDINALITY AS "l0" JOIN
/// UNNEST(tree) AS "l1" ON "l1"."<parent>" = "l0"."ordinality" … WHERE
/// filter)`.
///
/// Each element of `tree` names its parent by the 1-based position of the
/// parent element in the same array, so one level of the path is one unnest of
/// the array joined to the previous level by that position, and `filter` reads
/// each level's fields under the alias `levels` gives it. The first level is
/// left unconstrained: the caller states which elements a path starts from.
/// Only a level with a level below it needs its position, so the last one is
/// unnested without it, and its alias then names the element itself.
///
/// The `WITH ORDINALITY` source is no [`sea_query::TableRef`], so the
/// statement is a custom expression whose only interpolated pieces are
/// identifiers; `tree` and `filter` are rendered by sea-query in place of
/// `$1` and `$2`, which binds their values in statement order.
#[must_use]
pub fn exists_in_positional_tree(tree: SimpleExpr, levels: &[Alias], parent: &dyn Iden, filter: Condition) -> SimpleExpr {
    let sql = format!("EXISTS (SELECT 1 FROM {} WHERE $2)", positional_tree_source(levels, parent));
    Expr::cust_with_exprs(sql, [tree, filter.into()])
}

/// The first element at the end of a path through a tree that passes
/// `filter`, as a value of the array's element type, or `NULL` when none
/// does: `(SELECT "lN" FROM … WHERE filter LIMIT 1)`, the source as
/// [`exists_in_positional_tree`] unnests it.
///
/// Elements come in array order, which is the written order of siblings.
#[must_use]
pub fn first_in_positional_tree(tree: SimpleExpr, levels: &[Alias], parent: &dyn Iden, filter: Condition) -> SimpleExpr {
    let last = levels.last().map(|level| quoted(level)).unwrap_or_default();
    let sql = format!("(SELECT {last} FROM {} WHERE $2 LIMIT 1)", positional_tree_source(levels, parent));
    Expr::cust_with_exprs(sql, [tree, filter.into()])
}

/// One field of a composite value: `(value)."field"`.
#[must_use]
pub fn composite_field(value: SimpleExpr, field: &dyn Iden) -> SimpleExpr {
    Expr::cust_with_expr(format!("($1).{}", quoted(field)), value)
}

/// The `FROM` items of a path through a positional tree, the tree rendered as
/// `$1`.
fn positional_tree_source(levels: &[Alias], parent: &dyn Iden) -> String {
    let mut sql = String::new();
    for (depth, level) in levels.iter().enumerate() {
        if depth > 0 {
            sql.push_str(" JOIN ");
        }
        sql.push_str("UNNEST($1)");
        if depth + 1 < levels.len() {
            sql.push_str(" WITH ORDINALITY");
        }
        sql.push_str(" AS ");
        sql.push_str(&quoted(level));
        if let Some(above) = depth.checked_sub(1).and_then(|above| levels.get(above)) {
            sql.push_str(" ON ");
            sql.push_str(&qualified(level, parent));
            sql.push_str(" = ");
            sql.push_str(&qualified(above, &Ordinality));
        }
    }
    sql
}

#[cfg(test)]
mod tests {
    use crate::{
        pg_type::PostgresType,
        unnest::{UnnestColumn, apply_column_alias, apply_with_ordinality, composite_field, exists_in_positional_tree, first_in_positional_tree, unnest_call},
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Condition, Expr, ExprTrait, PostgresQueryBuilder, Query, SimpleExpr};

    /// The projection set-write's fourteen columns, in the order the upsert
    /// binds them, so the rendered source can be asserted against the real one.
    fn projection_columns() -> Vec<UnnestColumn> {
        [
            ("entity_key", PostgresType::BigintArray),
            ("modified_at", PostgresType::TimestamptzArray),
            ("attribute_name_key", PostgresType::IntegerArray),
            ("attribute_kind", PostgresType::SmallintArray),
            ("value_kind", PostgresType::SmallintArray),
            ("value_date_time", PostgresType::TimestamptzArray),
            ("unit_code_id", PostgresType::SmallintArray),
            ("value_boolean", PostgresType::BooleanArray),
            ("dataset_id", PostgresType::TextArray),
            ("value_number", PostgresType::TextArray),
            ("value_string", PostgresType::TextArray),
            ("value_geometry", PostgresType::ByteaArray),
            ("value_object", PostgresType::JsonbArray),
            ("value_uri", PostgresType::TextArray),
        ]
        .into_iter()
        .map(|(name, array_type)| UnnestColumn::new(Alias::new(name), array_type))
        .collect()
    }

    #[test]
    fn unnest_call_casts_each_array_to_its_element_type_and_binds_it() {
        let columns = [
            UnnestColumn::new(Alias::new("a"), PostgresType::BigintArray),
            UnnestColumn::new(Alias::new("b"), PostgresType::TextArray),
        ];
        let call = unnest_call([(&columns[0], SimpleExpr::from(Expr::val("a"))), (&columns[1], SimpleExpr::from(Expr::val("b")))]);
        let (sql, values) = Query::select()
            .expr(Expr::cust("d.x"))
            .from_function(call, Alias::new("d"))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("UNNEST($1::bigint[], $2::text[])"), "{sql}");
        // Both arrays are sea-query-bound values, not interpolated text.
        assert_eq!(values.0.len(), 2, "{sql}");
    }

    /// A number reaches the projection through a `text[]` array cast in the
    /// statement, never widened to a float on the wire.
    #[test]
    fn the_numeric_column_binds_as_a_text_array_not_a_float_array() {
        let columns = [UnnestColumn::new(Alias::new("value_number"), PostgresType::TextArray)];
        let call = unnest_call([(&columns[0], SimpleExpr::from(Expr::val("n")))]);
        let (sql, _values) = Query::select()
            .expr(Expr::cust("d.value_number"))
            .from_function(call, Alias::new("d"))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("$1::text[]"), "{sql}");
        assert!(!sql.contains("float8"), "{sql}");
        assert!(!sql.contains("double precision"), "{sql}");
    }

    /// Every level after the first joins the previous one by the parent
    /// position, and the tree is rendered once per level.
    #[test]
    fn a_positional_tree_joins_each_level_to_its_parent_by_position() {
        let levels = [Alias::new("l0"), Alias::new("l1")];
        let filter = Condition::all().add(Expr::col((Alias::new("l1"), Alias::new("value"))).eq(5));
        let (sql, values) = Query::select()
            .expr(exists_in_positional_tree(
                Expr::col((Alias::new("ca"), Alias::new("tree"))),
                &levels,
                &Alias::new("parent"),
                filter,
            ))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT EXISTS (SELECT 1 FROM UNNEST("ca"."tree") WITH ORDINALITY AS "l0" "#,
                r#"JOIN UNNEST("ca"."tree") AS "l1" ON "l1"."parent" = "l0"."ordinality" "#,
                r#"WHERE "l1"."value" = $1)"#,
            )
        );
        assert_eq!(values.0.len(), 1, "{sql}");
    }

    /// The last level names the element itself, which is what the first
    /// match returns.
    #[test]
    fn the_first_element_of_a_path_is_the_last_level_itself() {
        let levels = [Alias::new("l0")];
        let sql = Query::select()
            .expr(composite_field(
                first_in_positional_tree(Expr::col(Alias::new("tree")), &levels, &Alias::new("parent"), Condition::all()),
                &Alias::new("value"),
            ))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT ((SELECT "l0" FROM UNNEST("tree") AS "l0" WHERE TRUE LIMIT 1))."value""#);
    }

    #[test]
    fn with_ordinality_names_every_column_and_the_ordinality() {
        let columns = projection_columns();
        let call = unnest_call(columns.iter().map(|column| (column, SimpleExpr::from(Expr::val("v")))));
        let source = Query::select()
            .expr(Expr::cust("d.entity_key"))
            .from_function(call, Alias::new("d"))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        let rewritten = apply_with_ordinality(source, &Alias::new("d"), &columns, &Alias::new("ordinality"));
        // The rewritten set-write names its column list and ordinality instead
        // of the closed TableRef's quoted alias.
        assert!(!rewritten.contains("AS \"d\""), "{rewritten}");
        assert!(
            rewritten.contains(
                "WITH ORDINALITY AS d(entity_key, modified_at, attribute_name_key, attribute_kind, value_kind, value_date_time, unit_code_id, value_boolean, dataset_id, value_number, value_string, value_geometry, value_object, value_uri, ordinality)"
            ),
            "{rewritten}"
        );
    }

    #[test]
    fn a_column_alias_names_every_column_without_an_ordinality() {
        let columns = [
            UnnestColumn::new(Alias::new("write_id"), PostgresType::UuidArray),
            UnnestColumn::new(Alias::new("base_ordinal"), PostgresType::IntegerArray),
        ];
        let call = unnest_call([(&columns[0], SimpleExpr::from(Expr::val("a"))), (&columns[1], SimpleExpr::from(Expr::val("b")))]);
        let source = Query::select()
            .expr(Expr::cust("input.write_id"))
            .from_function(call, Alias::new("input"))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        let rewritten = apply_column_alias(source, &Alias::new("input"), &columns);
        assert!(!rewritten.contains("AS \"input\""), "{rewritten}");
        assert!(rewritten.contains("AS input(write_id, base_ordinal)"), "{rewritten}");
        assert!(!rewritten.contains("WITH ORDINALITY"), "{rewritten}");
    }

    #[test]
    fn the_ordinality_rewrite_leaves_the_bound_parameters_in_place() {
        let columns = [
            UnnestColumn::new(Alias::new("a"), PostgresType::BigintArray),
            UnnestColumn::new(Alias::new("b"), PostgresType::TextArray),
        ];
        let call = unnest_call([(&columns[0], SimpleExpr::from(Expr::val("a"))), (&columns[1], SimpleExpr::from(Expr::val("b")))]);
        let (sql, values) = Query::select()
            .expr(Expr::cust("d.x"))
            .from_function(call, Alias::new("stage"))
            .to_owned()
            .build(PostgresQueryBuilder);
        let rewritten = apply_with_ordinality(sql, &Alias::new("stage"), &columns, &Alias::new("ordinality"));
        assert!(rewritten.contains("$1::bigint[]") && rewritten.contains("$2::text[]"), "{rewritten}");
        assert_eq!(values.0.len(), 2, "{rewritten}");
    }
}
