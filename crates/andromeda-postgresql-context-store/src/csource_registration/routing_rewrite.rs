//! The wholesale rewrite of a registration's routing rows.
//!
//! [`crate::csource_registration::information_projection`] derives the rows a
//! registration's `information` yields; this module replaces the stored set
//! with them, inside the transaction that writes the registration payload.

use crate::{
    csource_registration::information_projection::{RegistrationInformationRow, derive_routing, projected_attribute_names},
    error::ContextStoreError,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::registration::info::RegistrationInfo;
use andromeda_ngsi_ld_query::pattern::Pattern;
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_schema::csr::RegistrationInformation;
use sea_query::{Expr, ExprTrait, PostgresQueryBuilder, Query};
use sea_query_sqlx::{SqlxBinder, SqlxValues};
use sqlx::PgConnection;

/// The delete half of the wholesale rewrite: every row of one registration.
fn delete_rows_statement(registration: &Uri) -> (String, SqlxValues) {
    Query::delete()
        .from_table(RegistrationInformation::Table)
        .and_where(Expr::col(RegistrationInformation::RegistrationId).eq(registration.as_str()))
        .build_sqlx(PostgresQueryBuilder)
}

/// The insert half of the wholesale rewrite: one `VALUES` row per routing row,
/// every array bound as one typed value.
fn insert_rows_statement(registration: &Uri, rows: &[RegistrationInformationRow<'_>]) -> Result<(String, SqlxValues), ContextStoreError> {
    let mut insert = Query::insert();
    insert.into_table(RegistrationInformation::Table).columns([
        RegistrationInformation::RegistrationId,
        RegistrationInformation::EntityTypes,
        RegistrationInformation::EntityId,
        RegistrationInformation::EntityIdPattern,
        RegistrationInformation::PropertyNameKeys,
        RegistrationInformation::RelationshipNameKeys,
    ]);
    for row in rows {
        let (properties, relationships) = row.attributes.stored();
        insert
            .values([
                Expr::val(registration.as_str()),
                Expr::val(row.types.stored()),
                Expr::val(row.identifier.stored()),
                Expr::val(row.identifier_pattern.map(Pattern::as_str)),
                Expr::val(properties),
                Expr::val(relationships),
            ])
            .map_err(|error| ContextStoreError::Internal(error.to_string()))?;
    }
    Ok(insert.build_sqlx(PostgresQueryBuilder))
}

/// Replace a registration's routing rows with the ones its `information` yields.
///
/// Runs on the caller's connection so that it joins the transaction that writes
/// the registration payload: the projection and the document it is derived from
/// become visible together or not at all. A failure here aborts the mutation.
///
/// The rewrite is wholesale -- one delete of the registration's rows followed by
/// one insert of the derived set -- because an update carries a resolved value
/// set rather than a fragment, so rows for capabilities the update dropped must
/// not survive it. Every registration has at least one row after it: the
/// unconstrained one has the wildcard row.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the attribute-name dictionary cannot
/// resolve or mint the declared names, when the insert statement cannot be built
/// from the derived rows, or when the delete or the insert fails against the
/// caller's connection.
pub async fn replace_registration_information(
    connection: &mut PgConnection,
    registration: &Uri,
    information: &[RegistrationInfo],
    attribute_names: &dyn NameLookup<AttributeName>,
) -> Result<(), ContextStoreError> {
    // Resolved on the caller's connection, so the dictionary rows this mints
    // belong to the transaction that writes the registration payload: the
    // routing projection and the names it is keyed by land together or not at
    // all.
    let ids = attribute_names
        .resolve_or_create(&mut *connection, &projected_attribute_names(information))
        .await?;
    let rows = derive_routing(information, &ids).into_rows();

    let (delete_sql, delete_values) = delete_rows_statement(registration);
    sqlx::query_with(sqlx::AssertSqlSafe(delete_sql.as_str()), delete_values)
        .execute(&mut *connection)
        .await?;

    let (insert_sql, insert_values) = insert_rows_statement(registration, &rows)?;
    sqlx::query_with(sqlx::AssertSqlSafe(insert_sql.as_str()), insert_values)
        .execute(connection)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::{
        information_projection::{RegistrationInformationRow, RegistrationRouting, derive_routing},
        routing_rewrite::insert_rows_statement,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
    use andromeda_ngsi_ld_model::{
        entity_info::{EntityInfo, EntityTypeSelection},
        registration::info::RegistrationInfo,
    };
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    /// An expanded attribute IRI, as the service layer hands one to the store.
    const SPEED: &str = "https://example.com/speed";

    /// The insert binds every array as one typed value per row and writes the
    /// sentinels where a row constrains nothing.
    #[test]
    fn the_insert_writes_one_values_row_per_routing_row_with_typed_array_binds() {
        let registration: Uri = "urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("valid URI");
        let speed: AttributeName = SPEED.parse().expect("valid attribute name");
        let dictionary = HashMap::from([(speed.clone(), AttributeNameKey::from(11))]); // clone: the dictionary key is owned beside the declared name.
        let information = vec![RegistrationInfo {
            entities: Some(vec![EntityInfo {
                id: None,
                id_pattern: None,
                type_: EntityTypeSelection::Single("Vehicle".parse().expect("valid type name")),
            }]),
            property_names: Some(vec![speed]),
            relationship_names: None,
        }];
        let mut rows = match derive_routing(&information, &dictionary) {
            RegistrationRouting::Unconstrained => panic!("the fixture constrains the type axis"),
            RegistrationRouting::Selective(rows) => rows,
        };
        rows.push(RegistrationInformationRow::wildcard());
        let (sql, values) = insert_rows_statement(&registration, &rows).expect("the insert builds");
        assert!(
            sql.starts_with(
                r#"INSERT INTO "registration_information" ("registration_id", "entity_types", "entity_id", "entity_id_pattern", "property_name_keys", "relationship_name_keys") VALUES ($1, $2, $3, $4, $5, $6), ($7, $8, $9, $10, $11, $12)"#
            ),
            "{sql}"
        );
        assert_eq!(values.0.0.len(), 12, "two rows of six bound values");
        let rendered = format!("{:?}", values.0.0);
        assert!(rendered.contains("\"*\""), "the wildcard row stores the type sentinel: {rendered}");
        assert!(rendered.contains("\"\""), "the wildcard row stores the identifier sentinel: {rendered}");
    }
}
