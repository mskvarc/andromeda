//! Decoding an [`Entity`] and an [`EntityPatch`] member by member.
//!
//! The derived decoding of a struct with a flattened attribute map buffers
//! every member it does not name -- every attribute -- in serde's generic
//! content before decoding it, which for a `GeoProperty` of a million
//! positions is a copy of the whole geometry per position token. Both types
//! are therefore read member by member: the members clause 5.2.4 names
//! decode as the derived decoding decodes them, and each attribute member is
//! read by a [`MemberReading`], from any deserializer as a JSON tree, or from
//! the text of a request as the slice of text it is.
//!
//! A member written twice counts as written last, as a parsed tree keeps it.

use crate::{
    attribute::instance::AttributeValue,
    entity::{Entity, EntityPatch},
    entity_member::{EntityForm, EntityMember, SharedMembers},
};
use serde::{
    Deserialize,
    Deserializer,
    de::{DeserializeSeed, Error as DeserializeError, MapAccess, Visitor},
};
use serde_json::value::RawValue;
use std::{fmt, marker::PhantomData};

/// How the attribute members of an entity are read.
pub(crate) trait MemberReading {
    /// Read the value of the member the map stands at as an attribute member.
    ///
    /// # Errors
    ///
    /// Returns the reader's failure.
    fn member<'de, A: MapAccess<'de>>(members: &mut A) -> Result<AttributeValue, A::Error>;

    /// Read the value of the member the map stands at as an attribute member
    /// a `null` leaves unset.
    ///
    /// # Errors
    ///
    /// Returns the reader's failure.
    fn optional_member<'de, A: MapAccess<'de>>(members: &mut A) -> Result<Option<AttributeValue>, A::Error>;
}

/// Reads each attribute member through its own decoding, from any
/// deserializer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FromAnyDeserializer;

impl MemberReading for FromAnyDeserializer {
    fn member<'de, A: MapAccess<'de>>(members: &mut A) -> Result<AttributeValue, A::Error> {
        members.next_value()
    }

    fn optional_member<'de, A: MapAccess<'de>>(members: &mut A) -> Result<Option<AttributeValue>, A::Error> {
        members.next_value()
    }
}

/// Reads each attribute member from the slice of request text it is, which
/// only `serde_json`'s reader over that text hands out.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FromRequestText;

impl MemberReading for FromRequestText {
    fn member<'de, A: MapAccess<'de>>(members: &mut A) -> Result<AttributeValue, A::Error> {
        AttributeValue::from_member_text(members.next_value::<&'de RawValue>()?).map_err(A::Error::custom)
    }

    fn optional_member<'de, A: MapAccess<'de>>(members: &mut A) -> Result<Option<AttributeValue>, A::Error> {
        members
            .next_value::<Option<&'de RawValue>>()?
            .map(AttributeValue::from_member_text)
            .transpose()
            .map_err(A::Error::custom)
    }
}

// Hand-written: see the module documentation.
impl<'de> Deserialize<'de> for Entity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Entity, D::Error> {
        deserializer.deserialize_map(EntityVisitor::<FromAnyDeserializer>::new())
    }
}

// Hand-written: see the module documentation.
impl<'de> Deserialize<'de> for EntityPatch {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<EntityPatch, D::Error> {
        deserializer.deserialize_map(EntityPatchVisitor::<FromAnyDeserializer>::new())
    }
}

/// Reads one member name of an entity.
#[derive(Clone, Copy, Debug)]
struct EntityMemberSeed {
    /// The form whose member names are read.
    form: EntityForm,
}

impl<'de> DeserializeSeed<'de> for EntityMemberSeed {
    type Value = EntityMember;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<EntityMember, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for EntityMemberSeed {
    type Value = EntityMember;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an entity member name")
    }

    fn visit_str<E: DeserializeError>(self, name: &str) -> Result<EntityMember, E> {
        EntityMember::named(name, self.form).map_err(E::custom)
    }
}

/// Read every member of the map as `form` names them, each attribute member by
/// `M`.
fn read_members<'de, M: MemberReading, A: MapAccess<'de>>(mut members: A, form: EntityForm) -> Result<SharedMembers, A::Error> {
    let mut read = SharedMembers::default();
    while let Some(member) = members.next_key_seed(EntityMemberSeed { form })? {
        match (member, form) {
            (EntityMember::Id, EntityForm::Entity) => read.id = Some(members.next_value()?),
            (EntityMember::Id, EntityForm::Patch) => read.id = members.next_value()?,
            (EntityMember::Type, EntityForm::Entity) => read.type_ = Some(members.next_value()?),
            (EntityMember::Type, EntityForm::Patch) => read.type_ = members.next_value()?,
            (EntityMember::Scope, EntityForm::Entity | EntityForm::Patch) => read.scope = members.next_value()?,
            (EntityMember::ExpiresAt, EntityForm::Entity | EntityForm::Patch) => read.expires_at = members.next_value()?,
            (EntityMember::Location, EntityForm::Entity | EntityForm::Patch) => read.location = M::optional_member(&mut members)?,
            (EntityMember::ObservationSpace, EntityForm::Entity | EntityForm::Patch) => read.observation_space = M::optional_member(&mut members)?,
            (EntityMember::OperationSpace, EntityForm::Entity | EntityForm::Patch) => read.operation_space = M::optional_member(&mut members)?,
            (EntityMember::CreatedAt, EntityForm::Entity | EntityForm::Patch) => read.created_at = members.next_value()?,
            (EntityMember::ModifiedAt, EntityForm::Entity | EntityForm::Patch) => read.modified_at = members.next_value()?,
            (EntityMember::DeletedAt, EntityForm::Entity | EntityForm::Patch) => read.deleted_at = members.next_value()?,
            (EntityMember::Attribute(name), EntityForm::Entity | EntityForm::Patch) => {
                let value = M::member(&mut members)?;
                read.attrs.insert(name, value);
            }
        }
    }
    Ok(read)
}

/// Reads one [`Entity`], each attribute member by `M`.
#[derive(Debug)]
pub(crate) struct EntityVisitor<M> {
    /// How attribute members are read.
    reading: PhantomData<M>,
}

impl<M> EntityVisitor<M> {
    /// A visitor reading attribute members by `M`.
    pub(crate) const fn new() -> EntityVisitor<M> {
        EntityVisitor { reading: PhantomData }
    }
}

impl<'de, M: MemberReading> Visitor<'de> for EntityVisitor<M> {
    type Value = Entity;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("struct Entity")
    }

    fn visit_map<A: MapAccess<'de>>(self, members: A) -> Result<Entity, A::Error> {
        read_members::<M, A>(members, EntityForm::Entity)?
            .into_entity()
            .map_err(|missing| A::Error::missing_field(missing.name()))
    }
}

/// Reads one [`EntityPatch`], each attribute member by `M`.
#[derive(Debug)]
pub(crate) struct EntityPatchVisitor<M> {
    /// How attribute members are read.
    reading: PhantomData<M>,
}

impl<M> EntityPatchVisitor<M> {
    /// A visitor reading attribute members by `M`.
    pub(crate) const fn new() -> EntityPatchVisitor<M> {
        EntityPatchVisitor { reading: PhantomData }
    }
}

impl<'de, M: MemberReading> Visitor<'de> for EntityPatchVisitor<M> {
    type Value = EntityPatch;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("struct EntityPatch")
    }

    fn visit_map<A: MapAccess<'de>>(self, members: A) -> Result<EntityPatch, A::Error> {
        read_members::<M, A>(members, EntityForm::Patch).map(SharedMembers::into_patch)
    }
}
