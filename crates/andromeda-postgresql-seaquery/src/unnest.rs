//! The `UNNEST(array, …)` set-write table source, with the column-list alias
//! and `WITH ORDINALITY` sea-query cannot spell.
//!
//! A set-based write turns one parallel array per column into a table and
//! inserts every row of it in a single statement, in batch order. sea-query's
//! [`sea_query::TableRef`] is a closed enum with no `WITH ORDINALITY` variant
//! and no column-list alias, and [`sea_query::SelectStatement::from_function`]
//! force-quotes the alias, so this source cannot be a native table reference.
//!
//! Every bound value stays inside sea-query anyway: [`unnest_call`] builds the
//! `UNNEST(…)` function call with each array cast to its element type, which
//! `from_function` binds and numbers like any other statement value. Only the
//! alias it renders, `AS "alias"`, becomes `AS alias(col, …)` or `WITH
//! ORDINALITY AS alias(col, …, ordinality)`: [`UnnestAliased`] carries the
//! statement and its aliases, and spells them when the statement is rendered.
//! The suffix holds identifiers only, so no bound value moves and the
//! statement's parameter numbering is untouched.

use crate::{
    cast::cast,
    identifier::{quoted_column, quoted_iden, quoted_name},
    pg_type::PostgresType,
    statement::{Statement, rendering::Rendered},
};
use sea_query::{Alias, Condition, DynIden, Expr, Func, FunctionCall, Iden, IntoIden, SimpleExpr, Values};

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

/// How one `UNNEST` table source is aliased.
#[derive(Clone)]
enum SourceAlias {
    /// `AS alias(col, …)`: each `UNNEST` argument a named row column.
    Columns {
        /// The source's alias.
        alias: DynIden,
        /// The row columns, in argument order.
        columns: Vec<DynIden>,
    },
    /// `WITH ORDINALITY AS alias(col, …, ordinality)`: the row columns and
    /// the 1-based row position.
    ColumnsWithOrdinality {
        /// The source's alias.
        alias: DynIden,
        /// The row columns, in argument order.
        columns: Vec<DynIden>,
        /// The column the row position is read under.
        ordinality: DynIden,
    },
    /// `WITH ORDINALITY AS "alias"` on every unnest of an array of
    /// composites under `alias`: the rows keep the composite's field names,
    /// and the position is [`Ordinality`]. A statement may unnest under one
    /// alias in several of its queries, each a scope of its own.
    RecordsWithOrdinality {
        /// The source's alias.
        alias: DynIden,
    },
}

impl SourceAlias {
    /// `sql` with the `AS "alias"` its `UNNEST` source renders spelled as
    /// this alias.
    ///
    /// The alias is one the statement renders nowhere else as `AS "alias"`:
    /// a set-write reads its columns as `alias.column`.
    fn apply(&self, sql: &str) -> String {
        let emitted = |alias: &DynIden| format!("AS {}", quoted_iden(alias));
        let names = |columns: &[DynIden]| columns.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
        match self {
            SourceAlias::Columns { alias, columns } => sql.replacen(&emitted(alias), &format!("AS {alias}({})", names(columns)), 1),
            SourceAlias::ColumnsWithOrdinality { alias, columns, ordinality } => {
                sql.replacen(&emitted(alias), &format!("WITH ORDINALITY AS {alias}({}, {ordinality})", names(columns)), 1)
            }
            SourceAlias::RecordsWithOrdinality { alias } => sql.replace(&emitted(alias), &format!("WITH ORDINALITY {}", emitted(alias))),
        }
    }
}

/// A statement whose `UNNEST` table sources are aliased as sea-query cannot
/// spell.
pub struct UnnestAliased<S> {
    /// The statement, each `UNNEST` source under its quoted alias.
    statement: S,
    /// The aliases spelled when it is rendered.
    aliases: Vec<SourceAlias>,
}

impl<S> UnnestAliased<S> {
    /// `statement`, its `UNNEST` sources not yet aliased.
    pub const fn new(statement: S) -> UnnestAliased<S> {
        UnnestAliased {
            statement,
            aliases: Vec::new(),
        }
    }

    /// The source under `alias` named `alias(col, …)`.
    #[must_use]
    pub fn with_columns(mut self, alias: impl IntoIden, columns: &[UnnestColumn]) -> UnnestAliased<S> {
        self.aliases.push(SourceAlias::Columns {
            alias: alias.into_iden(),
            columns: column_names(columns),
        });
        self
    }

    /// The source under `alias` named `WITH ORDINALITY AS alias(col, …,
    /// ordinality)`.
    #[must_use]
    pub fn with_ordinality(mut self, alias: impl IntoIden, columns: &[UnnestColumn], ordinality: impl IntoIden) -> UnnestAliased<S> {
        self.aliases.push(SourceAlias::ColumnsWithOrdinality {
            alias: alias.into_iden(),
            columns: column_names(columns),
            ordinality: ordinality.into_iden(),
        });
        self
    }

    /// Every unnest of an array of composites under `alias` gains its
    /// position, [`Ordinality`].
    #[must_use]
    pub fn with_record_ordinality(mut self, alias: impl IntoIden) -> UnnestAliased<S> {
        self.aliases.push(SourceAlias::RecordsWithOrdinality { alias: alias.into_iden() });
        self
    }
}

/// The names of `columns`.
fn column_names(columns: &[UnnestColumn]) -> Vec<DynIden> {
    // clone: each alias owns the names it spells.
    columns.iter().map(|column| column.name.clone()).collect()
}

impl<S: Rendered> Rendered for UnnestAliased<S> {
    fn rendered(&self) -> (String, Values) {
        let (sql, values) = self.statement.rendered();
        let sql = self.aliases.iter().fold(sql, |sql, alias| alias.apply(&sql));
        (sql, values)
    }
}

impl<S: Statement> Statement for UnnestAliased<S> {}

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
    let last = levels.last().map(|level| quoted_name(level)).unwrap_or_default();
    let sql = format!("(SELECT {last} FROM {} WHERE $2 LIMIT 1)", positional_tree_source(levels, parent));
    Expr::cust_with_exprs(sql, [tree, filter.into()])
}

/// One field of a composite value: `(value)."field"`.
#[must_use]
pub fn composite_field(value: SimpleExpr, field: &dyn Iden) -> SimpleExpr {
    Expr::cust_with_expr(format!("($1).{}", quoted_name(field)), value)
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
        sql.push_str(&quoted_name(level));
        if let Some(above) = depth.checked_sub(1).and_then(|above| levels.get(above)) {
            sql.push_str(" ON ");
            sql.push_str(&quoted_column(level, parent));
            sql.push_str(" = ");
            sql.push_str(&quoted_column(above, &Ordinality));
        }
    }
    sql
}

#[cfg(test)]
mod tests {
    use crate::{
        pg_type::PostgresType,
        statement::rendering::Rendered,
        unnest::{UnnestAliased, UnnestColumn, composite_field, exists_in_positional_tree, first_in_positional_tree, unnest_call, unnest_of},
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Condition, Expr, ExprTrait, PostgresQueryBuilder, Query, SimpleExpr, UnionType};

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
        let source = Query::select().expr(Expr::cust("d.entity_key")).from_function(call, Alias::new("d")).to_owned();
        let (rewritten, _) = UnnestAliased::new(source)
            .with_ordinality(Alias::new("d"), &columns, Alias::new("ordinality"))
            .rendered();
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
            .to_owned();
        let (rewritten, _) = UnnestAliased::new(source).with_columns(Alias::new("input"), &columns).rendered();
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
        let source = Query::select().expr(Expr::cust("d.x")).from_function(call, Alias::new("stage")).to_owned();
        let (rewritten, values) = UnnestAliased::new(source)
            .with_ordinality(Alias::new("stage"), &columns, Alias::new("ordinality"))
            .rendered();
        assert!(rewritten.contains("$1::bigint[]") && rewritten.contains("$2::text[]"), "{rewritten}");
        assert_eq!(values.0.len(), 2, "{rewritten}");
    }

    #[test]
    fn an_unnested_record_array_keeps_its_field_names_beside_its_position() {
        let sql = Query::select()
            .expr(Expr::col((Alias::new("e"), Alias::new("entry_kind"))))
            .from_as(Alias::new("intents"), Alias::new("i"))
            .from_function(unnest_of(Expr::col((Alias::new("i"), Alias::new("entries")))), Alias::new("e"))
            .to_owned();
        let (sql, _) = UnnestAliased::new(sql).with_record_ordinality(Alias::new("e")).rendered();
        assert!(sql.contains(r#"FROM "intents" AS "i", UNNEST("i"."entries") WITH ORDINALITY AS "e""#), "{sql}");
    }

    #[test]
    fn every_unnest_of_one_record_alias_gains_its_position() {
        let unnested = || {
            Query::select()
                .expr(Expr::col((Alias::new("e"), Alias::new("entry_kind"))))
                .from_function(unnest_of(Expr::col(Alias::new("entries"))), Alias::new("e"))
                .to_owned()
        };
        let sql = unnested().union(UnionType::All, unnested()).to_owned();
        let (sql, _) = UnnestAliased::new(sql).with_record_ordinality(Alias::new("e")).rendered();
        assert_eq!(sql.matches(r#"UNNEST("entries") WITH ORDINALITY AS "e""#).count(), 2, "{sql}");
    }
}
