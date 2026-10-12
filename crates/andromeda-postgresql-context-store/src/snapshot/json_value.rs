//! Encoding a typed value as the `json` a snapshot statement writes.

use crate::json_text::optional_json_text;
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use sea_query::{Expr, Keyword, SimpleExpr};
use serde::Serialize;

/// The statement value for one nullable `json` column: the value's JSON text
/// cast to `json`, or SQL `NULL` when there is none.
///
/// The text is written by the value's own `Serialize` impl and kept by the
/// column as written, so no JSON tree is built on the way.
///
/// # Errors
///
/// Returns the serializer's error when the value cannot be written as JSON.
pub(crate) fn optional_json<T: Serialize + ?Sized>(value: Option<&T>) -> Result<SimpleExpr, serde_json::Error> {
    Ok(optional_json_text(value)?.map_or(SimpleExpr::Keyword(Keyword::Null), |text| cast(Expr::val(text), PostgresType::Json)))
}

#[cfg(test)]
mod tests {
    use crate::snapshot::json_value::optional_json;
    use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    fn rendered(value: Option<&[KeyValuePair]>) -> String {
        let expression = optional_json(value).expect("the value serialises");
        Query::select().expr(expression).to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_present_value_is_written_as_its_json_text_cast_to_json() {
        let receiver_info = [KeyValuePair {
            key: "fiware-service".into(),
            value: "openiot".into(),
        }];
        assert_eq!(rendered(Some(&receiver_info)), r#"SELECT '[{"key":"fiware-service","value":"openiot"}]'::json"#);
    }

    #[test]
    fn an_absent_value_is_sql_null() {
        assert_eq!(rendered(None), "SELECT NULL");
    }
}
