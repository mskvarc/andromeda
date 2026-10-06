//! Encoding a typed value as the `jsonb` a snapshot statement writes.

use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use sea_query::{Expr, Keyword, SimpleExpr};
use serde::Serialize;

/// The statement value for one nullable `jsonb` column: the value's JSON text
/// cast to `jsonb`, or SQL `NULL` when there is none.
///
/// The text is written by the value's own `Serialize` impl, so no JSON tree is
/// built on the way to the column.
///
/// # Errors
///
/// Returns the serializer's error when the value cannot be written as JSON.
pub(crate) fn optional_jsonb<T: Serialize + ?Sized>(value: Option<&T>) -> Result<SimpleExpr, serde_json::Error> {
    match value {
        Some(value) => Ok(cast(Expr::val(serde_json::to_string(value)?), PostgresType::Jsonb)),
        None => Ok(SimpleExpr::Keyword(Keyword::Null)),
    }
}

#[cfg(test)]
mod tests {
    use crate::snapshot::jsonb_value::optional_jsonb;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    fn rendered(value: Option<&[u8]>) -> String {
        let expression = optional_jsonb(value).expect("the value serialises");
        Query::select().expr(expression).to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_present_value_is_written_as_its_json_text_cast_to_jsonb() {
        assert_eq!(rendered(Some(&[1, 2])), "SELECT '[1,2]'::jsonb");
    }

    #[test]
    fn an_absent_value_is_sql_null() {
        assert_eq!(rendered(None), "SELECT NULL");
    }
}
