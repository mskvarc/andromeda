//! The Context Registry's view of possibly available types and attributes
//! (clause 5.7.11), read off the registration information rows.
//!
//! `registration_information` holds every registration's `information` as one
//! row per `EntityInfo`, with the type names and the interned attribute-name
//! keys of its item beside each other, so the union clause 5.7.11 lets a
//! discovery take from the registry is one scan of the rows joined to the
//! attribute-name dictionary.

use crate::{
    csource_registration::registration_store::PostgresRegistrationStore,
    store_fault::{contract_broken, persistence_error, undecodable},
};
use andromeda_entity::{
    error::EntityError,
    port::registry_types::{RegisteredAttribute, RegisteredType, RegistryTypeSource},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_context_query::csr::{
    information_row::{ri, ri_col},
    preselect::{ANY_TYPE, entity_types_or_wildcard},
};
use andromeda_postgresql_schema::{
    csr::{Csr, RegistrationInformation},
    name_dictionary::AttributeNames,
};
use andromeda_postgresql_seaquery::{clock::now, pg_array::any, statement::Statement};
use async_trait::async_trait;
use sea_query::{Alias, Cond, Expr as SeaExpr, ExprTrait, JoinType, Query, SelectStatement, extension::postgres::PgExpr};

/// One information row joined to the dictionary: the row's type names and one
/// attribute name its item declares, or none when it declares no attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredPair {
    /// The row's `entity_types`, or the wildcard sentinel for an item without
    /// `entities`.
    pub entity_types: Vec<String>,
    /// One declared attribute name, or `None` for a row declaring none.
    pub attribute_name: Option<String>,
}

/// `SELECT coalesce(ri.entity_types, '{*}'), an.name FROM
/// registration_information ri JOIN context_source_registrations c ON c.id =
/// ri.registration_id LEFT JOIN attribute_names an ON an.attribute_name_key =
/// ANY(ri.property_name_keys || ri.relationship_name_keys)
/// WHERE c.expires_at IS NULL OR c.expires_at > NOW()`.
///
/// Array concatenation reads a `NULL` axis as empty, and two `NULL` axes give
/// a `NULL` array no key equals, so a row declaring no name joins to `NULL`
/// and still reports its types.
fn registered_pairs_sql() -> SelectStatement {
    let an = Alias::new("an");
    let declared_keys = ri_col(RegistrationInformation::PropertyNameKeys).concatenate(ri_col(RegistrationInformation::RelationshipNameKeys));
    let mut select = Query::select();
    select
        .expr(entity_types_or_wildcard())
        .column((an.clone(), AttributeNames::Name))
        .from_as(RegistrationInformation::Table, ri())
        .join(
            JoinType::Join,
            Csr::Table,
            SeaExpr::col((Csr::Table, Csr::Id)).equals((ri(), RegistrationInformation::RegistrationId)),
        )
        .join_as(
            JoinType::LeftJoin,
            AttributeNames::Table,
            an.clone(),
            SeaExpr::col((an, AttributeNames::AttributeNameKey)).eq(any(declared_keys)),
        )
        .cond_where(
            Cond::any()
                .add(SeaExpr::col((Csr::Table, Csr::ExpiresAt)).is_null())
                .add(SeaExpr::expr(SeaExpr::col((Csr::Table, Csr::ExpiresAt))).gt(now())),
        );
    select
}

/// Read a stored type name; the registration write stored it, so a malformed one is an
/// internal fault.
fn stored_type_name(raw: &str) -> Result<EntityTypeName, EntityError> {
    raw.parse().map_err(|error| EntityError::from(undecodable(&error)))
}

/// Read a stored attribute name; the dictionary wrote it, so a malformed one is
/// an internal fault.
fn stored_attribute_name(raw: &str) -> Result<AttributeName, EntityError> {
    raw.parse().map_err(|error| EntityError::from(undecodable(&error)))
}

/// Fold the joined rows into one entry per registered type, each with the
/// attribute names registered beside it, without repetition and in first-seen
/// order. The wildcard sentinel names no type and contributes nothing.
///
/// # Errors
///
/// Returns [`EntityError::internal`] when a stored entity type or attribute name
/// does not parse back into its lexicon type, which can only happen if the
/// rows hold a value the dictionary never minted.
pub fn registered_types_of(pairs: &[RegisteredPair]) -> Result<Vec<RegisteredType>, EntityError> {
    let mut types: Vec<RegisteredType> = Vec::new();
    for pair in pairs {
        let attribute = pair.attribute_name.as_deref().map(stored_attribute_name).transpose()?;
        for raw in pair.entity_types.iter().filter(|raw| raw.as_str() != ANY_TYPE) {
            let name = stored_type_name(raw)?;
            let entry = if let Some(entry) = types.iter_mut().find(|entry| entry.name == name) {
                entry
            } else {
                types.push(RegisteredType {
                    name,
                    attribute_names: Vec::new(),
                });
                types.last_mut().ok_or_else(|| EntityError::from(contract_broken()))?
            };
            if let Some(attribute) = &attribute
                && !entry.attribute_names.contains(attribute)
            {
                entry.attribute_names.push(attribute.clone());
            }
        }
    }
    Ok(types)
}

/// Fold the joined rows into one entry per registered attribute, each with the
/// entity types registered beside it, without repetition and in first-seen
/// order. The wildcard sentinel names no type and contributes nothing.
///
/// # Errors
///
/// Returns [`EntityError::internal`] when a stored attribute name or entity type
/// does not parse back into its lexicon type, which can only happen if the
/// rows hold a value the dictionary never minted.
pub fn registered_attributes_of(pairs: &[RegisteredPair]) -> Result<Vec<RegisteredAttribute>, EntityError> {
    let mut attributes: Vec<RegisteredAttribute> = Vec::new();
    for pair in pairs {
        let Some(raw_attribute) = pair.attribute_name.as_deref() else {
            continue;
        };
        let name = stored_attribute_name(raw_attribute)?;
        let entry = if let Some(entry) = attributes.iter_mut().find(|entry| entry.name == name) {
            entry
        } else {
            attributes.push(RegisteredAttribute {
                name,
                entity_types: Vec::new(),
            });
            attributes.last_mut().ok_or_else(|| EntityError::from(contract_broken()))?
        };
        for raw in pair.entity_types.iter().filter(|raw| raw.as_str() != ANY_TYPE) {
            let entity_type = stored_type_name(raw)?;
            if !entry.entity_types.contains(&entity_type) {
                entry.entity_types.push(entity_type);
            }
        }
    }
    Ok(attributes)
}

impl PostgresRegistrationStore {
    /// Every (entity types, attribute name) pair the live registrations
    /// declare, as their information rows hold them.
    async fn registered_pairs(&self) -> Result<Vec<RegisteredPair>, EntityError> {
        let rows: Vec<(Vec<String>, Option<String>)> = registered_pairs_sql()
            .to_query_as()
            .fetch_all(&self.pool)
            .await
            .map_err(|error| persistence_error::<EntityError>(error))?;
        Ok(rows
            .into_iter()
            .map(|(entity_types, attribute_name)| RegisteredPair { entity_types, attribute_name })
            .collect())
    }
}

#[async_trait]
impl RegistryTypeSource for PostgresRegistrationStore {
    async fn registered_types(&self) -> Result<Vec<RegisteredType>, EntityError> {
        registered_types_of(&self.registered_pairs().await?)
    }

    async fn registered_attributes(&self) -> Result<Vec<RegisteredAttribute>, EntityError> {
        registered_attributes_of(&self.registered_pairs().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::PostgresQueryBuilder;

    fn pair(entity_types: &[&str], attribute_name: Option<&str>) -> RegisteredPair {
        RegisteredPair {
            entity_types: entity_types.iter().map(|raw| (*raw).to_owned()).collect(),
            attribute_name: attribute_name.map(str::to_owned),
        }
    }

    const VEHICLE: &str = "https://example.com/Vehicle";
    const AIRCRAFT: &str = "https://example.com/Aircraft";
    const SPEED: &str = "https://example.com/speed";
    const THRUST: &str = "https://example.com/thrust";

    #[test]
    fn the_statement_joins_the_rows_to_the_dictionary_and_skips_expired_registrations() {
        let rendered = registered_pairs_sql().to_string(PostgresQueryBuilder);
        // `PgExpr::contains` shadows `str::contains` on a `String`; the slice resolves the inherent method.
        let sql: &str = &rendered;
        assert!(sql.contains("FROM \"registration_information\" AS \"ri\""), "{sql}");
        assert!(sql.contains("SELECT COALESCE(\"ri\".\"entity_types\", '{*}'), \"an\".\"name\""), "{sql}");
        assert!(
            sql.contains("LEFT JOIN \"attribute_names\" AS \"an\" ON \"an\".\"attribute_name_key\" = ANY(\"ri\".\"property_name_keys\" || \"ri\".\"relationship_name_keys\")"),
            "{sql}"
        );
        assert!(
            sql.contains("\"context_source_registrations\".\"expires_at\" IS NULL OR \"context_source_registrations\".\"expires_at\" > NOW()"),
            "{sql}"
        );
    }

    /// Clause 5.7.11: a type registered without attributes is reported with
    /// none; the wildcard sentinel names no type.
    #[test]
    fn registered_types_fold_rows_per_type_without_the_wildcard_sentinel() {
        let pairs = [
            pair(&[VEHICLE], Some(SPEED)),
            pair(&[VEHICLE, AIRCRAFT], Some(THRUST)),
            pair(&[VEHICLE], Some(SPEED)),
            pair(&["*"], Some(THRUST)),
            pair(&[AIRCRAFT], None),
        ];
        let types = registered_types_of(&pairs).expect("types fold");
        pretty_assertions::assert_eq!(types.len(), 2);
        pretty_assertions::assert_eq!(types[0].name.as_str(), VEHICLE);
        pretty_assertions::assert_eq!(
            types[0].attribute_names.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec![SPEED, THRUST]
        );
        pretty_assertions::assert_eq!(types[1].name.as_str(), AIRCRAFT);
        pretty_assertions::assert_eq!(types[1].attribute_names.iter().map(AttributeName::as_str).collect::<Vec<_>>(), vec![THRUST]);
    }

    #[test]
    fn registered_attributes_fold_rows_per_attribute_with_their_types() {
        let pairs = [
            pair(&[VEHICLE], Some(SPEED)),
            pair(&["*"], Some(THRUST)),
            pair(&[AIRCRAFT], Some(THRUST)),
            pair(&[AIRCRAFT], None),
        ];
        let attributes = registered_attributes_of(&pairs).expect("attributes fold");
        pretty_assertions::assert_eq!(attributes.len(), 2);
        pretty_assertions::assert_eq!(attributes[0].name.as_str(), SPEED);
        pretty_assertions::assert_eq!(attributes[0].entity_types.iter().map(EntityTypeName::as_str).collect::<Vec<_>>(), vec![VEHICLE]);
        pretty_assertions::assert_eq!(attributes[1].name.as_str(), THRUST);
        pretty_assertions::assert_eq!(
            attributes[1].entity_types.iter().map(EntityTypeName::as_str).collect::<Vec<_>>(),
            vec![AIRCRAFT]
        );
    }
}
