//! PostgreSQL array constructs sea-query has no native form for.

use sea_query::{Expr, ExprTrait, Func, Iden, PostgresQueryBuilder, QueryStatementBuilder, SelectStatement, SimpleExpr};

/// `ARRAY(subquery)` where the subquery carries bound values, collecting a
/// single-column result set into one Postgres array.
///
/// Unlike [`array_subquery`], the subquery is embedded as a sub-statement rather
/// than rendered to text, so any request value inside it — a name to resolve
/// through a dictionary, a type to overlap — binds and numbers with the enclosing
/// statement rather than being inlined.
#[must_use]
pub fn array_of_subquery(subquery: SelectStatement) -> SimpleExpr {
    Expr::cust_with_exprs("ARRAY($1)", [SimpleExpr::SubQuery(None, Box::new(subquery.into_sub_query_statement()))])
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

/// `cardinality(array) > 0` - whether `array` holds any element.
///
/// A NULL array has no cardinality, so the comparison is unknown and the
/// enclosing `WHERE` drops the row: the answer an empty array gets.
#[must_use]
pub fn has_elements(array: SimpleExpr) -> SimpleExpr {
    SimpleExpr::from(Func::cust(Cardinality).args([array])).gt(SimpleExpr::Constant(0.into()))
}

/// The `nullif` function.
#[derive(Iden, Clone, Copy)]
#[iden = "nullif"]
struct NullIf;

/// `nullif(array, '{}')` - `array`, or NULL when it holds no element.
///
/// A sort key that ranks an empty array as an absent value: PostgreSQL orders
/// an empty array before every non-empty one, so only a NULL reaches the
/// position `NULLS LAST` gives it. The untyped literal takes the type of
/// `array`.
#[must_use]
pub fn elements_or_null(array: SimpleExpr) -> SimpleExpr {
    Func::cust(NullIf).args([array, Expr::cust("'{}'")]).into()
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

/// `ARRAY(subquery)` - the array-valued subquery constructor, which collects a
/// single-column result set into one Postgres array.
///
/// Distinct from `ARRAY[…]`: the operand is a `SELECT`, not an element list.
/// sea-query has no form for it, so the subquery is rendered and wrapped here.
/// The subquery's text is embedded rather than parameterised, so every operand
/// in it must be a column or function call — a value-free subquery, which is the
/// case for `ARRAY(SELECT jsonb_array_elements_text(col))`: no request value
/// crosses this boundary, only columns of the enclosing statement.
#[must_use]
pub fn array_subquery(subquery: &SelectStatement) -> SimpleExpr {
    Expr::cust(format!("ARRAY({})", subquery.to_string(PostgresQueryBuilder)))
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

/// A `text[]` whose elements may be SQL `NULL`, in the order given.
///
/// One bound array value cannot carry a `NULL` element (see
/// [`crate::placeholder`]), and the `ARRAY[…]` constructor cannot take
/// sea-query markers, whose template tokenizer reads `[…]` as a quoted
/// identifier. The array is therefore concatenated from one-element arrays:
/// each present element binds as its own parameter and each absent one is a
/// `NULL`, which binds nothing. The concatenation is a balanced tree, so its
/// nesting grows with the logarithm of the element count. Every present
/// element is a parameter, so this suits the elements of one row, never a
/// batch.
#[must_use]
pub fn nullable_text_array(values: &[Option<&str>]) -> SimpleExpr {
    match values {
        [] => Expr::cust("'{}'::text[]"),
        [value] => array_append(
            Expr::cust("'{}'::text[]"),
            match value {
                Some(value) => Expr::val(*value),
                None => Expr::cust("NULL"),
            },
        ),
        [..] => {
            let (head, tail) = values.split_at(values.len() / 2);
            Func::cust(ArrayCat).args([nullable_text_array(head), nullable_text_array(tail)]).into()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::pg_array::{
        all,
        any,
        array_append,
        array_position,
        array_positions,
        array_subquery,
        elements_or_null,
        has_elements,
        nullable_text_array,
        width_bucket,
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query, SimpleExpr};

    fn render_expr(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn an_element_test_compares_the_array_cardinality() {
        let sql = render_expr(has_elements(Expr::col(Alias::new("scope_paths"))));
        assert_eq!(sql, "SELECT cardinality(\"scope_paths\") > 0");
    }

    #[test]
    fn an_empty_array_reads_as_null() {
        let sql = render_expr(elements_or_null(Expr::col(Alias::new("scope_paths"))));
        assert_eq!(sql, "SELECT nullif(\"scope_paths\", '{}')");
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
    fn an_array_subquery_wraps_a_select_in_the_array_constructor() {
        let subquery = Query::select().expr(Expr::col(Alias::new("value"))).to_owned();
        let sql = render_expr(array_subquery(&subquery));
        assert!(sql.contains(r#"ARRAY(SELECT "value")"#), "{sql}");
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
    fn a_nullable_text_array_binds_present_elements_and_spells_absent_ones() {
        let values = [Some("urn:a"), None, Some("urn:b")];
        let (sql, bound) = Query::select().expr(nullable_text_array(&values)).to_owned().build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            "SELECT array_cat(array_append('{}'::text[], $1), array_cat(array_append('{}'::text[], NULL), array_append('{}'::text[], $2)))"
        );
        assert_eq!(bound.0.len(), 2);
    }

    #[test]
    fn an_empty_nullable_text_array_is_typed() {
        assert_eq!(render_expr(nullable_text_array(&[])), "SELECT '{}'::text[]");
    }

    /// The concatenation is balanced: 64 elements nest seven calls deep, not
    /// sixty-four.
    #[test]
    fn a_nullable_text_array_nests_logarithmically() {
        let values: Vec<Option<&str>> = (0..64).map(|index| if index % 2 == 0 { Some("urn:x") } else { None }).collect();
        let (sql, bound) = Query::select().expr(nullable_text_array(&values)).to_owned().build(PostgresQueryBuilder);
        let depth = sql
            .chars()
            .scan(0_i32, |open, character| {
                *open += match character {
                    '(' => 1,
                    ')' => -1,
                    _ => 0,
                };
                Some(*open)
            })
            .max();
        assert_eq!(depth, Some(7), "{sql}");
        assert_eq!(bound.0.len(), 32);
    }
}
