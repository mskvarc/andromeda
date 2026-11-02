//! The text of a statement the broker prepares, and the values its numbered
//! markers stand for.
//!
//! Under `plan_cache_mode = auto` PostgreSQL keeps a generic plan, made once
//! without the parameter values, only when its estimated cost is not above the
//! average of the custom plans before it. A generic plan reads a bound `LIMIT`
//! or `OFFSET` as a tenth of its input and a bound boolean as an unknown
//! predicate, so a statement carrying either never looks cheap enough and is
//! planned again on every execution. A row count of `LIMIT` or `OFFSET` and a
//! non-NULL boolean are written into the text instead: both are typed integers
//! and booleans, never text a caller wrote, and the few distinct values keep
//! the number of statement texts within the connection's statement cache.
//! Every other value binds to a numbered marker.

use derive_more::Display;
use sea_query::{QueryBuilder, SqlWriter, Value, Values};
use std::fmt::{self, Write};

/// Statement text under construction, with the values bound so far.
#[derive(Debug, Default, Display)]
#[display("{text}")]
pub(crate) struct StatementText {
    /// The text written so far, markers numbered `$1..` in value order.
    text: String,
    /// The bound values, in marker order.
    values: Vec<Value>,
}

impl StatementText {
    /// The finished text and its bound values, in marker order.
    pub(crate) fn into_parts(self) -> (String, Values) {
        (self.text, Values(self.values))
    }

    /// Whether the next value is the row count of a `LIMIT` or `OFFSET`
    /// clause, which sea-query writes as the keyword, a space and the value.
    fn at_row_count(&self) -> bool {
        self.text.ends_with(" LIMIT ") || self.text.ends_with(" OFFSET ")
    }
}

impl Write for StatementText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.text.write_str(text)
    }

    fn write_char(&mut self, character: char) -> fmt::Result {
        self.text.write_char(character)
    }
}

impl SqlWriter for StatementText {
    fn push_param<T: QueryBuilder>(&mut self, value: Value, _builder: &T) {
        if let Value::Bool(Some(flag)) = value {
            self.text.push_str(if flag { "TRUE" } else { "FALSE" });
        } else if let Value::BigUnsigned(Some(count)) = value
            && self.at_row_count()
        {
            self.text.push_str(&count.to_string());
        } else {
            self.values.push(value);
            self.text.push('$');
            self.text.push_str(&self.values.len().to_string());
        }
    }

    fn as_writer(&mut self) -> &mut dyn Write {
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::statement_text::StatementText;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query, SelectStatement, Value};

    fn rendered(select: &SelectStatement) -> (String, Vec<Value>) {
        let mut text = StatementText::default();
        select.build_collect_into(PostgresQueryBuilder, &mut text);
        let (sql, values) = text.into_parts();
        (sql, values.0)
    }

    #[test]
    fn a_limit_and_an_offset_are_written_into_the_text() {
        let (sql, values) = rendered(
            Query::select()
                .column(Alias::new("id"))
                .from(Alias::new("entities"))
                .and_where(Expr::col(Alias::new("id")).eq("urn:a"))
                .limit(20)
                .offset(40),
        );
        assert_eq!(sql, r#"SELECT "id" FROM "entities" WHERE "id" = $1 LIMIT 20 OFFSET 40"#);
        assert_eq!(values, vec![Value::String(Some("urn:a".to_owned()))]);
    }

    #[test]
    fn a_fence_offset_inside_a_subquery_is_written_into_the_text() {
        let fenced = Query::select().column(Alias::new("key")).from(Alias::new("staged")).offset(0).take();
        let (sql, values) = rendered(Query::select().column(Alias::new("key")).from_subquery(fenced, Alias::new("s")).limit(1));
        assert_eq!(sql, r#"SELECT "key" FROM (SELECT "key" FROM "staged" OFFSET 0) AS "s" LIMIT 1"#);
        assert_eq!(values, Vec::new());
    }

    #[test]
    fn a_boolean_is_written_into_the_text_and_numbering_skips_it() {
        let (sql, values) = rendered(
            Query::select()
                .expr(Expr::val(true))
                .from(Alias::new("entities"))
                .and_where(Expr::col(Alias::new("deleted")).eq(false))
                .and_where(Expr::col(Alias::new("id")).eq("urn:a")),
        );
        assert_eq!(sql, r#"SELECT TRUE FROM "entities" WHERE "deleted" = FALSE AND "id" = $1"#);
        assert_eq!(values, vec![Value::String(Some("urn:a".to_owned()))]);
    }

    #[test]
    fn a_null_boolean_stays_bound() {
        let (sql, values) = rendered(Query::select().expr(Expr::val(Option::<bool>::None)));
        assert_eq!(sql, "SELECT $1");
        assert_eq!(values, vec![Value::Bool(None)]);
    }

    #[test]
    fn an_unsigned_value_outside_a_row_count_stays_bound() {
        let (sql, values) = rendered(Query::select().expr(Expr::val(7_u64)));
        assert_eq!(sql, "SELECT $1");
        assert_eq!(values, vec![Value::BigUnsigned(Some(7))]);
    }
}
