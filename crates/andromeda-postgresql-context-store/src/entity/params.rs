//! The parallel bind vectors every entity statement is driven from.
//!
//! One entity or a thousand, the statements bind the same arrays, so this is
//! the only shape the write path builds. Everything that turns a
//! specification value into a column (the `ltree` rendering of a scope, the
//! EWKB encoding of a geometry) happens on the way in here. The entity's
//! attribute instances are staged apart, one row each
//! ([`StagedRows`](crate::current_attribute::staged_rows::StagedRows)).

use crate::{
    entity::{
        staged_geometry::{HoistedEwkb, StagedGeometry},
        staged_scopes::StagedScopes,
    },
    error::ContextStoreError,
};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_postgresql_encoding::array_literal::IntegerArrayLiteral;
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
use chrono::{DateTime, Utc};
use sqlx::types::Json;

/// Parallel column vectors for one fused entity statement.
///
/// Every integer array is carried as the PostgreSQL array literal a statement
/// casts to its column type ([`IntegerArrayLiteral`]), one per entity.
#[derive(Debug)]
pub struct PreparedEntityColumns {
    /// Entity identifiers.
    pub ids: Vec<Identifier>,
    /// Type dictionary-key arrays.
    pub entity_type_name_keys: Vec<IntegerArrayLiteral>,
    /// Attribute-name dictionary-key arrays.
    pub attribute_name_keys: Vec<IntegerArrayLiteral>,
    /// The attribute name of every attribute instance, as dictionary keys
    /// aligned with `attribute_instance_kinds`.
    pub attribute_instance_name_keys: Vec<IntegerArrayLiteral>,
    /// The stored family code of every attribute instance, aligned with
    /// `attribute_instance_name_keys`.
    pub attribute_instance_kinds: Vec<IntegerArrayLiteral>,
    /// Scope arrays, as label paths and as written.
    pub scopes: Vec<StagedScopes>,
    /// Default `location` geometries, as EWKB or kept.
    pub locations: Vec<StagedGeometry<HoistedEwkb>>,
    /// The instant after which every `location` instance has expired, when
    /// every instance carries one.
    pub location_expires_ats: Vec<Option<DateTime<Utc>>>,
    /// Entity expiration timestamps.
    pub expires_ats: Vec<Option<DateTime<Utc>>>,
    /// Earliest attribute-instance expiration timestamps.
    pub min_attribute_expires_ats: Vec<Option<DateTime<Utc>>>,
}

impl PreparedEntityColumns {
    /// Create an empty set of parallel SQL bind vectors sized for `len` rows.
    #[must_use]
    pub fn with_capacity(len: usize) -> PreparedEntityColumns {
        PreparedEntityColumns {
            ids: Vec::with_capacity(len),
            entity_type_name_keys: Vec::with_capacity(len),
            attribute_name_keys: Vec::with_capacity(len),
            attribute_instance_name_keys: Vec::with_capacity(len),
            attribute_instance_kinds: Vec::with_capacity(len),
            scopes: Vec::with_capacity(len),
            locations: Vec::with_capacity(len),
            location_expires_ats: Vec::with_capacity(len),
            expires_ats: Vec::with_capacity(len),
            min_attribute_expires_ats: Vec::with_capacity(len),
        }
    }

    /// The scope label paths as the `jsonb[]` parameter the entity statements
    /// bind.
    pub(crate) fn scope_paths_bind(&self) -> Vec<Option<Json<&[String]>>> {
        self.scopes.iter().map(StagedScopes::label_paths).collect()
    }

    /// The written Scopes as the `jsonb[]` parameter the entity statements
    /// bind.
    pub(crate) fn verbatim_scope_paths_bind(&self) -> Vec<Option<Json<&[String]>>> {
        self.scopes.iter().map(StagedScopes::written).collect()
    }

    /// Append one row's type keys, in declaration order.
    pub fn push_entity_type_name_keys(&mut self, keys: impl IntoIterator<Item = EntityTypeNameKey>) {
        self.entity_type_name_keys
            .push(IntegerArrayLiteral::of(keys.into_iter().map(|key| i64::from(key.get()))));
    }

    /// Append one row's attribute-name keys, in payload member order.
    pub fn push_attribute_name_keys(&mut self, keys: impl IntoIterator<Item = AttributeNameKey>) {
        self.attribute_name_keys
            .push(IntegerArrayLiteral::of(keys.into_iter().map(|key| i64::from(key.get()))));
    }

    /// Append one row's attribute instances as the two aligned arrays the
    /// search row stores them in. This is the only way to append them, so the
    /// arrays cannot fall out of step with each other.
    ///
    /// The instances are sorted by name key and family code first: the stored
    /// arrays are compared whole to decide whether a write rewrites the search
    /// row, so a payload that lists the same instances in another member order
    /// must encode to the same arrays.
    pub fn push_attribute_instances(&mut self, mut instances: Vec<(AttributeNameKey, AttributeKind)>) {
        instances.sort_unstable_by_key(|(name, kind)| (name.get(), kind.code()));
        let instances = instances;
        self.attribute_instance_name_keys
            .push(IntegerArrayLiteral::of(instances.iter().map(|(name, _)| i64::from(name.get()))));
        self.attribute_instance_kinds
            .push(IntegerArrayLiteral::of(instances.iter().map(|(_, kind)| i64::from(kind.code()))));
    }

    /// Encode one row's hoisted `location` geometry into its EWKB bind
    /// vector, so EWKB stays inside the store. A kept geometry is not encoded
    /// at all.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Encoding`] when the geometry cannot be
    /// encoded as EWKB.
    pub fn push_location(&mut self, location: StagedGeometry<&GeometryForm>) -> Result<(), ContextStoreError> {
        self.locations.push(location.encoded()?);
        Ok(())
    }
}

/// The texts of one literal column, in row order, as a statement binds them
/// into a `text[]` parameter.
pub(crate) fn literal_texts(column: &[IntegerArrayLiteral]) -> Vec<&str> {
    column.iter().map(IntegerArrayLiteral::as_str).collect()
}
