//! Reading the `registration_information` rows of a set of registrations, so
//! each registration's `information` is assembled from its rows alone.
//!
//! The rows of every registration a read returns are read in one statement by
//! registration identifier, and the names behind their keys are looked up once
//! for the whole read.

use crate::{
    csource_registration::{
        information_rows::{InformationRow, SelectorColumns, held_keys, information_of},
        stored_information_error::StoredInformationError,
        stored_registration_error::StoredRegistrationError,
    },
    error::ContextStoreError,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::info::RegistrationInfo;
use andromeda_postgresql_codes::entity_type_shape::EntityTypeShapeCode;
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use andromeda_postgresql_schema::csr::RegistrationInformation;
use andromeda_postgresql_seaquery::{pg_array::any, pg_type::PostgresType, placeholder::external_placeholder_as};
use sea_query::{Expr, ExprTrait, PostgresQueryBuilder, Query};
use sqlx::PgExecutor;
use std::{collections::HashMap, hash::BuildHasher};

/// One `registration_information` row as the database returns it.
#[derive(Debug, sqlx::FromRow)]
pub struct RawInformationRow {
    /// The owning registration.
    registration_id: String,
    /// The item position.
    item_ordinal: i32,
    /// The selector position.
    selector_ordinal: i32,
    /// The type names, or `NULL` for an item without `entities`.
    entity_types: Option<Vec<String>>,
    /// The type shape code.
    entity_type_shape: Option<i16>,
    /// The `id`.
    entity_id: Option<String>,
    /// The `idPattern` as written.
    entity_id_pattern: Option<String>,
    /// The pattern's restatement.
    entity_id_pattern_form: Option<String>,
    /// The `propertyNames` keys.
    property_name_keys: Option<Vec<i32>>,
    /// The `relationshipNames` keys.
    relationship_name_keys: Option<Vec<i32>>,
}

impl RawInformationRow {
    /// The row with its shape code decoded, borrowing its text.
    fn row(&self) -> Result<InformationRow<'_>, StoredInformationError> {
        let selector = match (&self.entity_types, self.entity_type_shape) {
            (None, None) => None,
            (Some(types), Some(shape)) => Some(SelectorColumns {
                types: types.iter().map(String::as_str).collect(),
                shape: EntityTypeShapeCode::try_from(shape).map_err(StoredInformationError::TypeShapeCode)?,
                id: self.entity_id.as_deref(),
                pattern: self.entity_id_pattern.as_deref(),
                pattern_form: self.entity_id_pattern_form.as_deref(),
            }),
            (Some(_), None) | (None, Some(_)) => {
                return Err(StoredInformationError::TypeShape {
                    item_ordinal: self.item_ordinal,
                    selector_ordinal: self.selector_ordinal,
                });
            }
        };
        Ok(InformationRow {
            item_ordinal: self.item_ordinal,
            selector_ordinal: self.selector_ordinal,
            selector,
            property_name_keys: stored_keys(self.property_name_keys.as_deref()),
            relationship_name_keys: stored_keys(self.relationship_name_keys.as_deref()),
        })
    }
}

/// One stored key array as keys.
fn stored_keys(keys: Option<&[i32]>) -> Option<Vec<AttributeNameKey>> {
    keys.map(|keys| keys.iter().copied().map(AttributeNameKey::from).collect())
}

/// The rows of the registrations the identifiers bound as `$1` name.
fn rows_select_sql() -> String {
    Query::select()
        .columns([
            RegistrationInformation::RegistrationId,
            RegistrationInformation::ItemOrdinal,
            RegistrationInformation::SelectorOrdinal,
            RegistrationInformation::EntityTypes,
            RegistrationInformation::EntityTypeShape,
            RegistrationInformation::EntityId,
            RegistrationInformation::EntityIdPattern,
            RegistrationInformation::EntityIdPatternForm,
            RegistrationInformation::PropertyNameKeys,
            RegistrationInformation::RelationshipNameKeys,
        ])
        .from(RegistrationInformation::Table)
        .and_where(Expr::col(RegistrationInformation::RegistrationId).eq(any(external_placeholder_as(1, PostgresType::TextArray))))
        .to_string(PostgresQueryBuilder)
}

/// The information rows of a set of registrations, by registration.
#[derive(Debug, Default)]
pub struct RegistrationInformationRows {
    /// Each registration's rows, in no particular order.
    by_registration: HashMap<Identifier, Vec<RawInformationRow>>,
}

impl RegistrationInformationRows {
    /// Read the rows of the registrations `ids` name.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when the read fails, and
    /// [`ContextStoreError::StoredRegistration`] when a row names a
    /// registration identifier that is not a URI.
    pub async fn read<'connection>(executor: impl PgExecutor<'connection>, ids: &[&str]) -> Result<RegistrationInformationRows, ContextStoreError> {
        if ids.is_empty() {
            return Ok(RegistrationInformationRows::default());
        }
        let rows: Vec<RawInformationRow> = sqlx::query_as(sqlx::AssertSqlSafe(rows_select_sql())).bind(ids).fetch_all(executor).await?;
        let mut by_registration: HashMap<Identifier, Vec<RawInformationRow>> = HashMap::new();
        for row in rows {
            let registration = row.registration_id.parse::<Identifier>().map_err(StoredRegistrationError::Id)?;
            by_registration.entry(registration).or_default().push(row);
        }
        Ok(RegistrationInformationRows { by_registration })
    }

    /// Every dictionary key the rows hold, each once.
    ///
    /// # Errors
    ///
    /// Returns [`StoredInformationError`] when a row's shape code does not
    /// decode.
    pub fn held_keys(&self) -> Result<Vec<AttributeNameKey>, StoredInformationError> {
        let rows = self
            .by_registration
            .values()
            .flatten()
            .map(RawInformationRow::row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(held_keys(&rows))
    }

    /// The `information` of the registration `id`, assembled from its rows.
    ///
    /// # Errors
    ///
    /// Returns [`StoredInformationError`] when its rows do not assemble, a
    /// registration without rows included.
    pub fn information_of<S: BuildHasher>(
        &self,
        id: &Identifier,
        names: &HashMap<AttributeNameKey, AttributeName, S>,
    ) -> Result<Vec<RegistrationInfo>, StoredInformationError> {
        let rows = self
            .by_registration
            .get(id)
            .map(|rows| rows.iter().map(RawInformationRow::row).collect::<Result<Vec<_>, _>>())
            .transpose()?
            .unwrap_or_default();
        information_of(&rows, names)
    }
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::information_read::rows_select_sql;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_rows_of_every_registration_a_read_returns_are_read_by_identifier() {
        assert_eq!(
            rows_select_sql(),
            r#"SELECT "registration_id", "item_ordinal", "selector_ordinal", "entity_types", "entity_type_shape", "entity_id", "entity_id_pattern", "entity_id_pattern_form", "property_name_keys", "relationship_name_keys" FROM "registration_information" WHERE "registration_id" = ANY($1::text[])"#
        );
    }
}
