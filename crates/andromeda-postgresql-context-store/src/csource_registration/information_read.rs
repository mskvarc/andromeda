//! Reading the `registration_information` rows of a set of registrations, so
//! each registration's `information` is assembled from its rows alone.
//!
//! The rows of every registration a read returns travel on the registration's
//! own row, as one array of the table's row type, and the names behind their
//! keys are looked up once for the whole read.

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
use andromeda_postgresql_context_query::csr::registration_select::InformationRows;
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use sea_query::Iden;
use sqlx::{Row, postgres::PgRow};
use std::{collections::HashMap, hash::BuildHasher};

/// One `registration_information` row as a value of the table's row type,
/// field for field in the table's column order.
#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "registration_information")]
pub struct RawInformationRow {
    /// The item position.
    item_ordinal: i32,
    /// The selector position.
    selector_ordinal: i32,
    /// The type shape code.
    entity_type_shape: Option<i16>,
    /// The owning registration.
    registration_id: String,
    /// The type names, or `NULL` for an item without `entities`.
    entity_types: Option<Vec<String>>,
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

/// The information rows of a set of registrations, by registration.
#[derive(Debug, Default)]
pub struct RegistrationInformationRows {
    /// Each registration's rows, in no particular order.
    by_registration: HashMap<Identifier, Vec<RawInformationRow>>,
}

impl RegistrationInformationRows {
    /// The rows each registration row of `rows` carries in its
    /// [`InformationRows`] column.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when the column does not
    /// decode, and [`ContextStoreError::StoredRegistration`] when a row names a
    /// registration identifier that is not a URI.
    pub fn of(rows: &[PgRow]) -> Result<RegistrationInformationRows, ContextStoreError> {
        let column = InformationRows.to_string();
        let mut by_registration: HashMap<Identifier, Vec<RawInformationRow>> = HashMap::new();
        for row in rows {
            for information in row.try_get::<Vec<RawInformationRow>, _>(column.as_str())? {
                let registration = information.registration_id.parse::<Identifier>().map_err(StoredRegistrationError::Id)?;
                by_registration.entry(registration).or_default().push(information);
            }
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
    use crate::migration_set::declared_schema::DeclaredSchema;
    use pretty_assertions::assert_eq;

    /// A row-type value decodes field by field in the table's column order,
    /// so the record keeps that order.
    #[test]
    fn an_information_record_follows_the_table_column_order() {
        let declared: Vec<String> = DeclaredSchema::of_every_migration()
            .columns
            .into_iter()
            .filter(|column| column.table == "registration_information")
            .map(|column| column.name)
            .collect();
        assert_eq!(
            declared,
            [
                "item_ordinal",
                "selector_ordinal",
                "entity_type_shape",
                "registration_id",
                "entity_types",
                "entity_id",
                "entity_id_pattern",
                "entity_id_pattern_form",
                "property_name_keys",
                "relationship_name_keys",
            ]
        );
    }
}
