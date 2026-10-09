//! Assembling an Entity's attributes from its `current_attributes` rows.
//!
//! Rows are read without an order, so they are put in written order by their
//! ordinal; the attributes take the order of their first instance. Each row's
//! sub-attribute tree is rebuilt from the parent positions of its elements,
//! last element first, so every element's children are complete before the
//! element itself is, with no recursion.
//!
//! A stored row that breaks an invariant no constraint can state (a parent at
//! or after its child, two siblings with one grain, a value of another
//! family) is refused with a typed error, so it is never rendered.

use crate::current_attribute::{
    name_keys::KeyNames,
    row::{CurrentAttributeRow, InstanceMembers},
    stored_family::{Envelope, StoredFamily},
    sub_attribute::SubAttributeElement,
};
use andromeda_ngsi_ld_entity::{
    attribute_instances::{AttributeInstances, RepeatedInstance},
    attributes::Attributes,
    date_time_text::DateTimeText,
    instance::Instance,
    literal::LiteralError,
    proof_seals::ProofSeals,
    refusal::EntityRefusal,
    system_instant::SystemInstant,
    unit_code::{StoredUnitCodeError, UnitCode},
};
use andromeda_ngsi_ld_geometry::geojson::GeometryError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_model::temporal::structured_value_codec::StoredValueError;
use andromeda_postgresql_codes::{attribute_kind_code::attribute_type_of, value_kind::ValueKind};
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use thiserror::Error;

/// Stored rows that do not assemble into an Entity's attributes.
#[derive(Debug, Error)]
pub enum AssemblyError {
    /// An element names a parent past the end of its array.
    #[error("element {position} names parent {parent}, outside its array")]
    ParentOutsideArray {
        /// The element's 1-based position.
        position: usize,
        /// The parent it names.
        parent: i32,
    },
    /// An element names itself or a later element as its parent.
    #[error("element {position} names parent {parent}, which does not precede it")]
    ParentOutOfOrder {
        /// The element's 1-based position.
        position: usize,
        /// The parent it names.
        parent: i32,
    },
    /// Two rows hold one instance grain (clause 4.5.5).
    #[error("two rows hold one instance: {0}")]
    RepeatedInstance(#[source] RepeatedInstance),
    /// Two sibling elements hold one instance grain (clause 4.5.5).
    #[error("two sub-attribute elements hold one instance: {0}")]
    RepeatedSubAttribute(#[source] RepeatedInstance),
    /// An attribute name key has no name.
    #[error("the attribute name key {} has no name", .0.get())]
    UnknownAttribute(AttributeNameKey),
    /// A type key has no name.
    #[error("the type key {} has no name", .0.get())]
    UnknownType(EntityTypeNameKey),
    /// The stored value is not one the instance's family holds.
    #[error("an instance of '{attribute}' stores a {} value its family {family} does not hold", .kind.label())]
    ValueOfAnotherFamily {
        /// The attribute.
        attribute: AttributeName,
        /// The instance's family.
        family: AttributeType,
        /// The stored value kind.
        kind: ValueKind,
    },
    /// A member is stored that the instance's family does not define.
    #[error("an instance of '{attribute}' stores a member its family {family} does not define")]
    ForeignMember {
        /// The attribute.
        attribute: AttributeName,
        /// The instance's family.
        family: AttributeType,
    },
    /// The structured value does not read as the value of its family.
    #[error(transparent)]
    StoredValue(Box<StoredValueError>),
    /// Geometry members or seals do not read as expanded members.
    #[error(transparent)]
    Refused(Box<EntityRefusal>),
    /// A structured value holds the NGSI-LD Null, which state never does.
    #[error("an instance of '{0}' stores the NGSI-LD Null")]
    NullInState(AttributeName),
    /// The unit code identity names no unit code.
    #[error(transparent)]
    UnitCode(#[from] StoredUnitCodeError),
    /// The stored clause 4.7.2 string is not a geometry.
    #[error(transparent)]
    EncodedGeometry(#[from] GeometryError),
    /// The stored `VocabProperty` term is not a name.
    #[error(transparent)]
    Term(#[from] NgsiLdReferenceError),
    /// The stored literal carries annotations its value cannot carry.
    #[error(transparent)]
    Literal(#[from] LiteralError),
}

impl From<StoredValueError> for AssemblyError {
    fn from(error: StoredValueError) -> AssemblyError {
        AssemblyError::StoredValue(Box::new(error))
    }
}

impl From<EntityRefusal> for AssemblyError {
    fn from(refusal: EntityRefusal) -> AssemblyError {
        AssemblyError::Refused(Box::new(refusal))
    }
}

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

/// The name behind an attribute name key.
fn attribute_name(key: AttributeNameKey, names: &KeyNames) -> Result<AttributeName, AssemblyError> {
    // clone: the assembled state owns its names
    names.attribute(key).cloned().ok_or(AssemblyError::UnknownAttribute(key))
}

/// One assembled instance: its attribute, its `datasetId` and the instance.
type AssembledInstance = (AttributeName, Option<Identifier>, Instance);

/// The sub-attributes a row's elements hold.
fn sub_attributes(elements: Vec<SubAttributeElement>, names: &KeyNames) -> Result<Attributes, AssemblyError> {
    if elements.is_empty() {
        return Ok(Attributes::new());
    }
    let mut children = vec![Vec::new(); elements.len() + 1];
    for (index, element) in elements.iter().enumerate() {
        let position = index + 1;
        let parent = usize::try_from(element.parent).map_err(|_| AssemblyError::ParentOutsideArray {
            position,
            parent: element.parent,
        })?;
        if parent > elements.len() {
            return Err(AssemblyError::ParentOutsideArray {
                position,
                parent: element.parent,
            });
        }
        if parent >= position {
            return Err(AssemblyError::ParentOutOfOrder {
                position,
                parent: element.parent,
            });
        }
        children[parent].push(position);
    }
    let mut built: Vec<Option<AssembledInstance>> = (0..=elements.len()).map(|_| None).collect();
    for (index, element) in elements.into_iter().enumerate().rev() {
        let position = index + 1;
        let own = grouped(take_children(&mut built, &children[position])).map_err(AssemblyError::RepeatedSubAttribute)?;
        let name = attribute_name(element.members.attribute_name_key, names)?;
        built[position] = Some(instance(name, element.members, (element.created_at, element.modified_at), own, names)?);
    }
    grouped(take_children(&mut built, &children[0])).map_err(AssemblyError::RepeatedSubAttribute)
}

/// The built instances at `positions`, in order.
///
/// Every child follows its parent, and elements are built last first, so each
/// child is built, and taken exactly once, before its parent takes it.
fn take_children(built: &mut [Option<AssembledInstance>], positions: &[usize]) -> Vec<AssembledInstance> {
    positions
        .iter()
        .filter_map(|position| built.get_mut(*position).and_then(Option::take))
        .collect()
}

/// Attributes of `instances`, each attribute in the order of its first
/// instance and its instances in order.
fn grouped(instances: Vec<AssembledInstance>) -> Result<Attributes, RepeatedInstance> {
    let mut by_name: IndexMap<AttributeName, Vec<(Option<Identifier>, Instance)>> = IndexMap::new();
    for (name, dataset_id, instance) in instances {
        by_name.entry(name).or_default().push((dataset_id, instance));
    }
    by_name
        .into_iter()
        .filter_map(|(name, instances)| {
            AttributeInstances::try_from_instances(instances)
                .transpose()
                .map(|held| held.map(|held| (name, held)))
        })
        .collect()
}

/// One instance of `name` from its members.
fn instance(
    name: AttributeName,
    members: InstanceMembers,
    (created_at, modified_at): (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
    sub_attributes: Attributes,
    names: &KeyNames,
) -> Result<AssembledInstance, AssemblyError> {
    let InstanceMembers {
        attribute_name_key: _,
        dataset_id,
        attribute_kind,
        value,
        value_type_key,
        object_type_keys,
        unit_code_id,
        observed_at,
        expires_at,
        proof_seals,
    } = members;
    let stored = StoredFamily {
        name: &name,
        family: attribute_type_of(attribute_kind),
        names,
    };
    let envelope = Envelope {
        unit_code: unit_code_id.map(UnitCode::from_id).transpose()?,
        value_type: value_type_key.map(|key| stored.type_name(key)).transpose()?,
        object_types: object_type_keys.into_iter().map(|key| stored.type_name(key)).collect::<Result<_, _>>()?,
    };
    let value = stored.value(value, envelope)?;
    let proof_seals = proof_seals
        .map(|seals| ProofSeals::from_stored(&name, seals.get()))
        .transpose()?
        .unwrap_or_default();
    let instance = Instance::builder()
        .value(value)
        .observed_at(observed_at.map(DateTimeText::from_instant))
        .expires_at(expires_at.map(DateTimeText::from_instant))
        .created_at(created_at.map(SystemInstant::new))
        .modified_at(modified_at.map(SystemInstant::new))
        .proof_seals(proof_seals)
        .sub_attributes(sub_attributes)
        .build();
    Ok((name, dataset_id, instance))
}
