//! PostgreSQL array constructs sea-query has no native form for.

use crate::literal::{false_literal, true_literal};
use sea_query::{Expr, ExprTrait, Func, Iden, Nullable, QueryStatementBuilder, SelectStatement, SimpleExpr, Value, ValueType};

/// `ARRAY(subquery)`: a single-column result set collected into one Postgres
/// array, which sea-query has no form for.
///
/// Distinct from `ARRAY[…]`: the operand is a `SELECT`, not an element list.
/// The subquery is embedded as a sub-statement, so any value inside it (a name
/// to resolve through a dictionary, a type to overlap) binds and numbers with
/// the enclosing statement.
#[must_use]
pub fn array_of_subquery(subquery: SelectStatement) -> SimpleExpr {
    Expr::cust_with_exprs("ARRAY $1", [SimpleExpr::SubQuery(None, Box::new(subquery.into_sub_query_statement()))])
}

/// The `ANY` array quantifier.
#[derive(Iden, Clone, Copy)]
#[iden = "ANY"]
struct AnyQuantifier;

/// The `ALL` array quantifier.
#[derive(Iden, Clone, Copy)]
#[iden = "ALL"]
struct AllQuantifier;

/// The `array_position` function.
#[derive(Iden, Clone, Copy)]
#[iden = "array_position"]
struct ArrayPosition;

/// `array_position(array, element)` - the one-based position of the first
/// occurrence of `element` in `array`, or NULL when it is absent.
///
/// The ordering key that reads a dictionary-keyed array back in its stored
/// order: a subquery over the dictionary is sorted by each key's position in
/// the row's own array, so a rendered type list comes back in the order the
/// entity declared it.
#[must_use]
pub fn array_position(array: SimpleExpr, element: SimpleExpr) -> SimpleExpr {
    Func::cust(ArrayPosition).args([array, element]).into()
}

/// The `cardinality` function.
#[derive(Iden, Clone, Copy)]
#[iden = "cardinality"]
struct Cardinality;

/// `(array)[position]` - the element of `array` at the 1-based `position`,
/// NULL past either end.
#[must_use]
pub fn array_element(array: SimpleExpr, position: u32) -> SimpleExpr {
    Expr::cust_with_exprs(format!("($1)[{position}]"), [array])
}

/// `(array)[position]` - the element of `array` at the 1-based position an
/// expression computes, NULL past either end and for a NULL position.
#[must_use]
pub fn array_subscript(array: SimpleExpr, position: SimpleExpr) -> SimpleExpr {
    // sea-query substitutes nothing inside a bracketed span of a template, so
    // the brackets are operands of their own, as in `array_literal`.
    Expr::cust_with_exprs("$1 $2 $3 $4 $5", [Expr::cust("("), array, Expr::cust(")["), position, Expr::cust("]")])
}

/// `ARRAY[element, ...]` - an array built from expressions, such as the
/// columns of the row a statement selects.
#[must_use]
pub fn array_literal(elements: impl IntoIterator<Item = SimpleExpr>) -> SimpleExpr {
    // sea-query reads a bracketed span of a custom fragment as one quoted
    // token and substitutes nothing inside it, so the brackets and separators
    // are operands of their own and the template is placeholders alone.
    let mut parts = vec![Expr::cust("ARRAY[")];
    for (position, element) in elements.into_iter().enumerate() {
        if position > 0 {
            parts.push(Expr::cust(","));
        }
        parts.push(element);
    }
    parts.push(Expr::cust("]"));
    let template = (1..=parts.len()).map(|position| format!("${position}")).collect::<Vec<_>>().join(" ");
    Expr::cust_with_exprs(template, parts)
}

/// `cardinality(array)` - the number of elements of `array`, NULL for a NULL
/// array.
#[must_use]
pub fn cardinality(array: SimpleExpr) -> SimpleExpr {
    Func::cust(Cardinality).args([array]).into()
}

/// `cardinality(array) > 0` - whether `array` holds any element.
///
/// A NULL array has no cardinality, so the comparison is unknown and the
/// enclosing `WHERE` drops the row: the answer an empty array gets.
#[must_use]
pub fn has_elements(array: SimpleExpr) -> SimpleExpr {
    SimpleExpr::from(Func::cust(Cardinality).args([array])).gt(SimpleExpr::Constant(0.into()))
}

/// The `array_append` function.
#[derive(Iden, Clone, Copy)]
#[iden = "array_append"]
struct ArrayAppend;

/// `array_append(array, element)` - `array` with `element` added at its end.
///
/// The way a routing predicate names a sentinel beside the values it resolved:
/// an overlap against `array_append(ARRAY(<resolved keys>), 0)` admits both the
/// rows declaring one of the keys and the rows whose stored set is the
/// wildcard `{0}`, in one operator a GIN index serves.
#[must_use]
pub fn array_append(array: SimpleExpr, element: SimpleExpr) -> SimpleExpr {
    Func::cust(ArrayAppend).args([array, element]).into()
}

/// The `width_bucket` function.
#[derive(Iden, Clone, Copy)]
#[iden = "width_bucket"]
struct WidthBucket;

/// `width_bucket(operand, thresholds)`: the position in the ascending array
/// `thresholds` of the last element at or before `operand`, 0 when every
/// element lies after it.
///
/// A binary search over the array, so placing a value among n bounds costs
/// about log n comparisons whatever the bounds are.
#[must_use]
pub fn width_bucket(operand: SimpleExpr, thresholds: SimpleExpr) -> SimpleExpr {
    Func::cust(WidthBucket).args([operand, thresholds]).into()
}

/// The `generate_subscripts` function.
#[derive(Iden, Clone, Copy)]
#[iden = "generate_subscripts"]
struct GenerateSubscripts;

/// `generate_subscripts(array, 1)`: the positions of a one-dimensional
/// array's elements, a set-returning function.
///
/// Projected beside `unnest(array)` it yields each element's position in step
/// with the element, which an array subscript cannot do here: a sea-query
/// template reads `[…]` as a quoted identifier.
#[must_use]
pub fn array_positions(array: SimpleExpr) -> SimpleExpr {
    Func::cust(GenerateSubscripts).args([array, SimpleExpr::Constant(1.into())]).into()
}

/// `array_cat(left, right)`: the elements of `left`, then those of `right`.
#[must_use]
pub fn array_cat(left: SimpleExpr, right: SimpleExpr) -> SimpleExpr {
    Func::cust(ArrayCat).args([left, right]).into()
}

/// `ANY(array)` - the right operand of `column = ANY(array)`.
///
/// Membership is tested against one bound array rather than expanding to a
/// placeholder per element, so a large batch cannot exhaust the wire protocol's
/// parameter space. The array binds as a single value.
#[must_use]
pub fn any(array: SimpleExpr) -> SimpleExpr {
    Func::cust(AnyQuantifier).args([array]).into()
}

/// `ALL(array)` - the right operand of `column <> ALL(array)`, bound as one
/// value for the reason [`any`] binds its array as one.
#[must_use]
pub fn all(array: SimpleExpr) -> SimpleExpr {
    Func::cust(AllQuantifier).args([array]).into()
}

/// `operand = ANY($n)`: whether `operand` is one of `values`.
///
/// Every list a request supplies (entity identifiers, attribute names,
/// `datasetId`s, the dictionary keys they resolve to) is tested this way, so
/// however long the list grows the statement binds it as one value and stays
/// far below the wire protocol's 65535-parameter ceiling. An empty list matches
/// nothing and renders as the constant `FALSE`, which PostgreSQL folds into a
/// filter that reads no row. The array owns its elements, so a text list is
/// handed over as owned strings: a bound parameter cannot borrow across the
/// round trip.
///
/// A list of one value renders as `operand = $n`. A generic plan estimates
/// `= ANY($n)` as ten elements whatever the array holds, so for the one key a
/// statement mostly carries it never looks as cheap as the custom plans and the
/// statement is planned again on every execution; `= $n` estimates one row,
/// which the generic plan shares with them.
#[must_use]
pub fn is_any_of<V>(operand: impl Into<SimpleExpr>, values: impl IntoIterator<Item = V>) -> SimpleExpr
where
    V: Into<Value>,
    Vec<V>: Into<Value>,
{
    match Membership::of(values) {
        Membership::Empty => false_literal(),
        Membership::Single(value) => operand.into().eq(Expr::val(value)),
        Membership::Several(values) => operand.into().eq(any(Expr::val(values))),
    }
}

/// `operand <> ALL($n)`: whether `operand` is none of `values`, the list bound
/// as one value for the reason [`is_any_of`] binds it as one. An empty list
/// excludes nothing and renders as the constant `TRUE`, and a list of one value
/// renders as `operand <> $n` for the reason [`is_any_of`] renders one as
/// `operand = $n`.
#[must_use]
pub fn is_none_of<V>(operand: impl Into<SimpleExpr>, values: impl IntoIterator<Item = V>) -> SimpleExpr
where
    V: Into<Value>,
    Vec<V>: Into<Value>,
{
    match Membership::of(values) {
        Membership::Empty => true_literal(),
        Membership::Single(value) => operand.into().ne(Expr::val(value)),
        Membership::Several(values) => operand.into().ne(all(Expr::val(values))),
    }
}

/// The values a membership test compares against, by how many there are.
enum Membership<V> {
    /// No value.
    Empty,
    /// Exactly one value, compared directly.
    Single(V),
    /// Two values or more, bound as one array.
    Several(Vec<V>),
}

impl<V> Membership<V> {
    /// The membership `values` make.
    fn of(values: impl IntoIterator<Item = V>) -> Membership<V> {
        let mut values = values.into_iter();
        let Some(first) = values.next() else {
            return Membership::Empty;
        };
        match values.next() {
            None => Membership::Single(first),
            Some(second) => Membership::Several([first, second].into_iter().chain(values).collect()),
        }
    }
}

/// A bound `text[]` value from a batch of borrowed strings.
///
/// The array value owns its elements, so each string is copied: a bound
/// parameter cannot borrow from the caller's slice across the round trip.
#[must_use]
pub fn text_array(values: &[&str]) -> SimpleExpr {
    Expr::val(values.iter().map(|value| (*value).to_owned()).collect::<Vec<String>>())
}

/// The `array_cat` function.
#[derive(Iden, Clone, Copy)]
#[iden = "array_cat"]
struct ArrayCat;

/// One bound array whose elements may be SQL `NULL`, in the order given.
///
/// The whole array is one parameter however long it is, an absent element a
/// NULL element of it, so a column of a parallel-array set-write, where a row
/// has no value, binds like any other array.
#[must_use]
pub fn nullable_array<T>(elements: impl IntoIterator<Item = Option<T>>) -> SimpleExpr
where
    T: Into<Value> + Nullable + ValueType,
{
    let elements = elements.into_iter().map(|element| element.map_or_else(T::null, Into::into)).collect();
    Expr::val(Value::Array(T::array_type(), Some(Box::new(elements))))
}

#[cfg(test)]
mod tests {
    use crate::pg_array::{
        all,
        any,
        array_append,
        array_element,
        array_literal,
        array_of_subquery,
        array_position,
        array_positions,
        array_subscript,
        cardinality,
        has_elements,
        is_any_of,
        is_none_of,
        nullable_array,
        width_bucket,
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, ArrayType, Expr, ExprTrait, PostgresQueryBuilder, Query, SimpleExpr, Value};

    fn render_expr(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    /// Seventy thousand identifiers are past the 65535 parameters one
    /// statement may bind, and still bind as the one array they are.
    #[test]
    fn a_membership_list_past_the_parameter_ceiling_binds_as_one_array() {
        let identifiers: Vec<String> = (0..70_000).map(|index| format!("urn:ngsi-ld:Vehicle:{index}")).collect();
        let (sql, values) = Query::select()
            .expr(is_any_of(Expr::col(Alias::new("id")), identifiers.iter().cloned()))
            .and_where(is_none_of(Expr::col(Alias::new("name")), ["a".to_owned(), "b".to_owned()]))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT \"id\" = ANY($1) WHERE \"name\" <> ALL($2)");
        assert_eq!(values.0.len(), 2);
        assert!(matches!(&values.0[0], Value::Array(_, Some(elements)) if elements.len() == identifiers.len()));
    }

    #[test]
    fn an_empty_list_is_a_constant() {
        let (sql, values) = Query::select()
            .expr(is_any_of(Expr::col(Alias::new("id")), Vec::<String>::new()))
            .expr(is_none_of(Expr::col(Alias::new("id")), Vec::<String>::new()))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT FALSE, TRUE");
        assert_eq!(values.0, Vec::new());
    }

    #[test]
    fn a_list_of_one_value_is_compared_directly() {
        let (sql, values) = Query::select()
            .expr(is_any_of(Expr::col(Alias::new("id")), ["urn:a".to_owned()]))
            .expr(is_none_of(Expr::col(Alias::new("key")), [7_i64]))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT \"id\" = $1, \"key\" <> $2");
        assert_eq!(values.0, vec![Value::String(Some("urn:a".to_owned())), Value::BigInt(Some(7))]);
    }

    #[test]
    fn integer_keys_bind_as_one_array_too() {
        let (sql, values) = Query::select()
            .expr(is_any_of(Expr::col(Alias::new("key")), [1_i32, 2, 3]))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT \"key\" = ANY($1)");
        assert_eq!(values.0.len(), 1);
    }

    #[test]
    fn an_element_is_read_at_its_one_based_position() {
        let sql = render_expr(array_element(Expr::col(Alias::new("events")), 1));
        assert_eq!(sql, "SELECT (\"events\")[1]");
    }

    #[test]
    fn a_subscript_reads_the_element_at_a_computed_position_of_a_bound_array() {
        let (sql, values) = Query::select()
            .expr(array_subscript(
                Expr::val(vec!["a".to_owned(), "b".to_owned()]),
                Expr::col(Alias::new("position")),
            ))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT ( $1 )[ \"position\" ]");
        assert_eq!(values.0.len(), 1);
    }

    #[test]
    fn an_array_literal_lists_its_element_expressions() {
        let sql = render_expr(array_literal([Expr::col(Alias::new("a")), Expr::col(Alias::new("b"))]));
        assert_eq!(sql, "SELECT ARRAY[ \"a\" , \"b\" ]");
    }

    #[test]
    fn the_cardinality_of_an_array_is_its_element_count() {
        let sql = render_expr(cardinality(Expr::col(Alias::new("sub_attributes"))));
        assert_eq!(sql, "SELECT cardinality(\"sub_attributes\")");
    }

    #[test]
    fn an_element_test_compares_the_array_cardinality() {
        let sql = render_expr(has_elements(Expr::col(Alias::new("scope_paths"))));
        assert_eq!(sql, "SELECT cardinality(\"scope_paths\") > 0");
    }

    #[test]
    fn array_position_renders_the_two_argument_function() {
        let sql = render_expr(array_position(
            Expr::col(Alias::new("entity_type_name_keys")),
            Expr::col(Alias::new("entity_type_name_key")),
        ));
        assert_eq!(sql, "SELECT array_position(\"entity_type_name_keys\", \"entity_type_name_key\")");
    }

    #[test]
    fn a_value_is_placed_among_ascending_thresholds_by_width_bucket() {
        let sql = render_expr(width_bucket(Expr::col(Alias::new("at")), Expr::col(Alias::new("starts"))));
        assert_eq!(sql, "SELECT width_bucket(\"at\", \"starts\")");
    }

    #[test]
    fn the_positions_of_an_array_are_its_first_dimension_subscripts() {
        let sql = render_expr(array_positions(Expr::col(Alias::new("starts"))));
        assert_eq!(sql, "SELECT generate_subscripts(\"starts\", 1)");
    }

    #[test]
    fn array_append_renders_the_two_argument_function() {
        let sql = render_expr(array_append(Expr::col(Alias::new("keys")), Expr::val(0)));
        assert_eq!(sql, "SELECT array_append(\"keys\", 0)");
    }

    #[test]
    fn an_array_subquery_wraps_a_select_in_the_array_constructor_and_binds_its_values() {
        let subquery = Query::select()
            .expr(Expr::col(Alias::new("value")))
            .and_where(Expr::col(Alias::new("key")).eq(3))
            .to_owned();
        let (sql, values) = Query::select().expr(array_of_subquery(subquery)).to_owned().build(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT ARRAY (SELECT "value" WHERE "key" = $1)"#);
        assert_eq!(values.0.len(), 1);
    }

    #[test]
    fn equality_against_any_binds_the_array_once() {
        let (sql, values) = Query::select()
            .column(Alias::new("attribute_name_key"))
            .from(Alias::new("attribute_names"))
            .and_where(Expr::col(Alias::new("name")).eq(any(Expr::val(vec!["a".to_string(), "b".to_string()]))))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("= ANY("), "{sql}");
        assert_eq!(values.0.len(), 1, "the whole array is one bound value: {sql}");
    }

    #[test]
    fn inequality_against_all_binds_the_array_once() {
        let (sql, values) = Query::select()
            .column(Alias::new("name"))
            .from(Alias::new("attribute_names"))
            .and_where(Expr::col(Alias::new("attribute_name_key")).ne(all(Expr::val(vec![3_i32, 9]))))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.ends_with(r#""attribute_name_key" <> ALL($1)"#), "{sql}");
        assert_eq!(values.0.len(), 1, "the whole array is one bound value: {sql}");
    }

    #[test]
    fn a_nullable_array_is_one_bound_value_holding_its_absent_elements() {
        let (sql, bound) = Query::select()
            .expr(nullable_array([Some("urn:a".to_owned()), None, Some("urn:b".to_owned())]))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT $1");
        assert_eq!(
            bound.0,
            vec![Value::Array(
                ArrayType::String,
                Some(Box::new(vec![
                    Value::String(Some("urn:a".to_owned())),
                    Value::String(None),
                    Value::String(Some("urn:b".to_owned()))
                ]))
            )]
        );
    }
}
