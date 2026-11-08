//! The system times a broker writes (`createdAt`, `modifiedAt`, `deletedAt`, clause 4.8), read from an entity or
//! from an attribute or sub-attribute of it, so a test can hold a later read to the times an earlier one answered.

use ngsi_ld::model::{Attribute, Attributes, DateTime, Entity, EntityId, Output};

/// One system member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum SystemMember {
    /// `createdAt`.
    #[strum(serialize = "createdAt")]
    CreatedAt,
    /// `modifiedAt`.
    #[strum(serialize = "modifiedAt")]
    ModifiedAt,
    /// `deletedAt`.
    #[strum(serialize = "deletedAt")]
    DeletedAt,
}

/// Why a system time could not be read.
#[derive(Debug, thiserror::Error)]
pub enum SystemTimeError {
    /// No attribute of the name the path gives at that level, the first level being 1.
    #[error("{entity} holds no attribute at level {level} of the path")]
    NoAttribute {
        /// The entity read.
        entity: EntityId,
        /// The level of the path.
        level: usize,
    },
    /// The attribute holds several instances, so which one is meant is not said.
    #[error("the attribute at level {level} of the path in {entity} holds {count} instances")]
    Instances {
        /// The entity read.
        entity: EntityId,
        /// The level of the path, the first being 1.
        level: usize,
        /// How many instances it holds.
        count: usize,
    },
    /// The member is not written on what the path names, the entity itself for an empty path.
    #[error("{entity} carries no {member} at depth {depth} of the path")]
    Missing {
        /// The entity read.
        entity: EntityId,
        /// How many attribute levels the path descends.
        depth: usize,
        /// The member read.
        member: SystemMember,
    },
}

/// The `member` of `entity`.
///
/// # Errors
///
/// Returns [`SystemTimeError::Missing`] when the entity does not carry it.
pub fn of_entity(entity: &Entity<Output>, member: SystemMember) -> Result<&DateTime, SystemTimeError> {
    let output = &entity.output;
    match member {
        SystemMember::CreatedAt => output.created_at.as_ref(),
        SystemMember::ModifiedAt => output.modified_at.as_ref(),
        SystemMember::DeletedAt => output.deleted_at.as_ref(),
    }
    .ok_or_else(|| SystemTimeError::Missing {
        // clone: the error must own the entity id it reports
        entity: entity.id.clone(),
        depth: 0,
        member,
    })
}

/// The `member` of the attribute at `path` of `entity`: an attribute name, then the names of the sub-attributes
/// below it. Each level must hold one instance.
///
/// # Errors
///
/// Returns [`SystemTimeError`] when a level is missing or holds several instances, or the member is not written.
pub fn of_attribute<'e>(entity: &'e Entity<Output>, path: &[&str], member: SystemMember) -> Result<&'e DateTime, SystemTimeError> {
    let attribute = path.iter().zip(1..).try_fold(None::<&Attribute<Output>>, |parent, (name, level)| {
        let attributes = parent.map_or(&entity.attributes, |attribute| &attribute.meta().attributes);
        single(attributes, name).map(Some).map_err(|shortfall| shortfall.of(entity, level))
    })?;
    let Some(attribute) = attribute else {
        return of_entity(entity, member);
    };
    let output = &attribute.meta().output;
    match member {
        SystemMember::CreatedAt => output.created_at.as_ref(),
        SystemMember::ModifiedAt => output.modified_at.as_ref(),
        SystemMember::DeletedAt => output.deleted_at.as_ref(),
    }
    .ok_or_else(|| SystemTimeError::Missing {
        // clone: the error must own the entity id it reports
        entity: entity.id.clone(),
        depth: path.len(),
        member,
    })
}

/// Why one level of a path names no single instance.
enum LevelShortfall {
    /// No attribute has the name.
    Absent,
    /// The attribute holds this many instances.
    Several(usize),
}

impl LevelShortfall {
    /// The error this shortfall is at `level` of a path into `entity`.
    fn of(self, entity: &Entity<Output>, level: usize) -> SystemTimeError {
        // clone: the error must own the entity id it reports
        let entity = entity.id.clone();
        match self {
            LevelShortfall::Absent => SystemTimeError::NoAttribute { entity, level },
            LevelShortfall::Several(count) => SystemTimeError::Instances { entity, level, count },
        }
    }
}

/// The one instance of the attribute `name` of `attributes`.
fn single<'a>(attributes: &'a Attributes<Output>, name: &str) -> Result<&'a Attribute<Output>, LevelShortfall> {
    let instances = attributes
        .iter()
        .find_map(|(candidate, instances)| (candidate.as_str() == name).then_some(instances))
        .ok_or(LevelShortfall::Absent)?;
    if instances.len() == 1 {
        Ok(instances.first())
    } else {
        Err(LevelShortfall::Several(instances.len()))
    }
}

#[cfg(test)]
mod tests {
    use crate::system_time::{SystemMember, of_attribute, of_entity};
    use googletest::prelude::*;
    use ngsi_ld::model::{DateTime, Entity, Lossless, Output};

    fn entity() -> Result<Entity<Output>> {
        Ok(Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","createdAt":"2024-01-01T00:00:00Z","modifiedAt":"2024-01-02T00:00:00Z",
            "speed":{"type":"Property","value":1,"modifiedAt":"2024-01-03T00:00:00Z",
            "accuracy":{"type":"Property","value":2,"createdAt":"2024-01-04T00:00:00Z"}}}"#,
            Lossless::Normalized,
        )?)
    }

    #[gtest]
    fn the_entity_and_each_attribute_level_answer_their_own_time() -> Result<()> {
        let entity = entity()?;
        expect_that!(of_entity(&entity, SystemMember::ModifiedAt)?, eq(&"2024-01-02T00:00:00Z".parse::<DateTime>()?));
        expect_that!(
            of_attribute(&entity, &["speed"], SystemMember::ModifiedAt)?,
            eq(&"2024-01-03T00:00:00Z".parse::<DateTime>()?)
        );
        verify_that!(
            of_attribute(&entity, &["speed", "accuracy"], SystemMember::CreatedAt)?,
            eq(&"2024-01-04T00:00:00Z".parse::<DateTime>()?)
        )
    }

    #[gtest]
    fn a_missing_level_or_member_is_an_error() -> Result<()> {
        let entity = entity()?;
        expect_that!(of_attribute(&entity, &["heading"], SystemMember::CreatedAt), err(anything()));
        verify_that!(of_attribute(&entity, &["speed"], SystemMember::DeletedAt), err(anything()))
    }
}
