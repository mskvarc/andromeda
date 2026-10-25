//! A subscription document as the `json` column value a statement writes.

use crate::{json_text::optional_json_text, store_fault::persistence_error};
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use andromeda_subscription::error::SubscriptionError;
use sea_query::{Expr, SimpleExpr};
use serde::Serialize;

/// The JSON text of `document` cast to `json`, `NULL` when the member is
/// absent.
///
/// # Errors
///
/// Returns the persistence failure of a document that cannot be written as
/// JSON.
pub(super) fn json_document<T: Serialize + ?Sized>(document: Option<&T>) -> Result<SimpleExpr, SubscriptionError> {
    let text = optional_json_text(document).map_err(|error| persistence_error::<SubscriptionError>(error))?;
    Ok(cast(Expr::val(text), PostgresType::Json))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::subscription_store::json_document::json_document;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn a_document_is_bound_as_text_and_cast_to_json() {
        let (sql, values) = Query::select()
            .expr(json_document(Some(&["a"])).expect("a serialisable document"))
            .build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, "SELECT $1::json");
        pretty_assertions::assert_eq!(values.0.len(), 1);
    }

    #[test]
    fn an_absent_document_is_a_null_json() {
        let (sql, values) = Query::select()
            .expr(json_document::<[&str]>(None).expect("no document"))
            .build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, "SELECT $1::json");
        pretty_assertions::assert_eq!(values.0.len(), 1);
    }
}
