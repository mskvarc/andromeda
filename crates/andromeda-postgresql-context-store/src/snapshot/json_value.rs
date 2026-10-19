//! Encoding a typed value as the `json` a snapshot statement writes.

use crate::{
    error::ContextStoreError,
    json_text::optional_json_text,
    snapshot::{persistence_error, unencodable_snapshot_error::UnencodableSnapshotError},
};
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use andromeda_snapshot::error::SnapshotError;
use sea_query::{Expr, Keyword, SimpleExpr};
use serde::Serialize;

/// The JSON text of one nullable Snapshot document, or `None` when there is
/// none.
///
/// # Errors
///
/// Returns the persistence failure an [`UnencodableSnapshotError`] built by
/// `unencodable` classifies as when the value cannot be written as JSON.
pub(crate) fn document_text<T: Serialize + ?Sized>(
    value: Option<&T>,
    unencodable: fn(serde_json::Error) -> UnencodableSnapshotError,
) -> Result<Option<String>, SnapshotError> {
    optional_json_text(value).map_err(|error| persistence_error(ContextStoreError::from(unencodable(error))))
}

/// The statement value for one nullable `json` column: the value's JSON text
/// cast to `json`, or SQL `NULL` when there is none.
///
/// The text is written by the value's own `Serialize` impl and kept by the
/// column as written, so no JSON tree is built on the way.
///
/// # Errors
///
/// Returns the persistence failure an [`UnencodableSnapshotError`] built by
/// `unencodable` classifies as when the value cannot be written as JSON.
pub(crate) fn optional_json<T: Serialize + ?Sized>(
    value: Option<&T>,
    unencodable: fn(serde_json::Error) -> UnencodableSnapshotError,
) -> Result<SimpleExpr, SnapshotError> {
    Ok(document_text(value, unencodable)?.map_or(SimpleExpr::Keyword(Keyword::Null), |text| cast(Expr::val(text), PostgresType::Json)))
}

#[cfg(test)]
mod tests {
    use crate::snapshot::{
        json_value::{document_text, optional_json},
        unencodable_snapshot_error::UnencodableSnapshotError,
    };
    use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_snapshot::error::SnapshotError;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};
    use serde::{Serialize, Serializer, ser::Error};

    /// A document whose serializer refuses it.
    struct Unserializable;

    impl Serialize for Unserializable {
        fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
            Err(S::Error::custom("refused"))
        }
    }

    fn rendered(value: Option<&[KeyValuePair]>) -> String {
        let expression = optional_json(value, UnencodableSnapshotError::ReceiverInfo).expect("the value serialises");
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

    #[test]
    fn a_document_that_does_not_serialize_fails_the_write_as_unencodable() {
        assert!(matches!(
            document_text(Some(&Unserializable), UnencodableSnapshotError::QueryContext),
            Err(SnapshotError::Persistence(PersistenceFailure::Unencodable))
        ));
        assert!(matches!(
            optional_json(Some(&Unserializable), UnencodableSnapshotError::ReceiverInfo),
            Err(SnapshotError::Persistence(PersistenceFailure::Unencodable))
        ));
    }
}
