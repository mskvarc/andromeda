//! Assembling an Entity's attributes from its `current_attributes` rows.
//!
//! Rows are read without an order, so they are put in written order by their
//! ordinal; the attributes take the order of their first instance.

use crate::current_attribute::row::CurrentAttributeRow;
use andromeda_ngsi_ld_entity::attributes::Attributes;
use andromeda_postgresql_attribute_instance::{
    assemble::{AssemblyError, attribute_name, grouped, instance, sub_attributes},
    name_keys::KeyNames,
};

/// The attributes an Entity's rows hold, in written order.
///
/// # Errors
///
/// Returns [`AssemblyError`] when a key has no name in `names`, or when the
/// rows break an invariant of the stored layout.
pub fn assemble(mut rows: Vec<CurrentAttributeRow>, names: &KeyNames) -> Result<Attributes, AssemblyError> {
    rows.sort_unstable_by_key(|row| row.ordinal);
    let mut instances = Vec::with_capacity(rows.len());
    for row in rows {
        let name = attribute_name(row.members.attribute_name_key, names)?;
        let sub_attributes = sub_attributes(row.sub_attributes, names)?;
        instances.push(instance(
            name,
            row.members,
            (Some(row.created_at), Some(row.modified_at)),
            sub_attributes,
            names,
        )?);
    }
    grouped(instances).map_err(AssemblyError::RepeatedInstance)
}
