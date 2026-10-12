//! Flattening an Entity's attributes into `current_attributes` rows.
//!
//! Each top-level attribute instance is one row, numbered by its written
//! position among the Entity's instances, holding its members and its
//! sub-attribute tree as elements. The state must be the one a write commits
//! (its values in their committed spelling, its top-level instances stamped),
//! since storage keeps nothing else.

use crate::current_attribute::row::CurrentAttributeRow;
use andromeda_ngsi_ld_entity::{attributes::Attributes, instance::Instance};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_postgresql_attribute_instance::{
    flatten::{FlattenError, members, push_elements},
    name_keys::NameKeys,
};

/// The rows of an Entity with `attributes`, in written order.
///
/// # Errors
///
/// Returns [`FlattenError`] when a name has no key in `keys`, or when the
/// attributes are not the ones a write commits.
pub fn flatten(attributes: &Attributes, keys: &NameKeys) -> Result<Vec<CurrentAttributeRow>, FlattenError> {
    let mut rows = Vec::with_capacity(attributes.len());
    for (name, instances) in attributes.iter() {
        for (dataset_id, instance) in instances.iter() {
            let ordinal = ordinal(name, rows.len())?;
            rows.push(flatten_instance(name, dataset_id, instance, ordinal, keys)?);
        }
    }
    Ok(rows)
}

/// The row of the top-level instance of `name` with `dataset_id`, at the
/// written position `ordinal`.
///
/// # Errors
///
/// Returns [`FlattenError`] when a name has no key in `keys`, or when the
/// instance is not one a write commits.
pub(crate) fn flatten_instance(
    name: &AttributeName,
    dataset_id: Option<&Identifier>,
    instance: &Instance,
    ordinal: i32,
    keys: &NameKeys,
) -> Result<CurrentAttributeRow, FlattenError> {
    let (Some(created_at), Some(modified_at)) = (instance.created_at(), instance.modified_at()) else {
        return Err(FlattenError::Unstamped(name.clone()));
    };
    let mut sub_attributes = Vec::new();
    push_elements(instance.sub_attributes(), 0, keys, &mut sub_attributes)?;
    Ok(CurrentAttributeRow {
        ordinal,
        members: members(name, dataset_id, instance, keys)?,
        created_at: created_at.get(),
        modified_at: modified_at.get(),
        sub_attributes,
    })
}

/// The row ordinal of the instance of `name` at the written `position`.
///
/// # Errors
///
/// Returns [`FlattenError::TooManyInstances`] past the range an ordinal counts.
pub(crate) fn ordinal(name: &AttributeName, position: usize) -> Result<i32, FlattenError> {
    i32::try_from(position).map_err(|_| FlattenError::TooManyInstances(name.clone()))
}
