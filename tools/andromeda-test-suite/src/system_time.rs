//! The system times a broker writes (`createdAt`, `modifiedAt`, `deletedAt`, clause 4.8), read from an entity or
//! from an attribute or sub-attribute of it, so a test can hold a later read to the times an earlier one answered.

use ngsi_ld::model::{Attribute, Attributes, DateTime, Entity, Output};

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
    /// No attribute of that name at that level.
    #[error("there is no attribute {0}")]
    NoAttribute(String),
    /// The attribute holds several instances, so which one is meant is not said.
    #[error("the attribute {0} holds {1} instances")]
    Instances(String, usize),
    /// The member is not written.
    #[error("{0} carries no {1}")]
    Missing(String, SystemMember),
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
    .ok_or_else(|| SystemTimeError::Missing(entity.id.to_string(), member))
}

/// The `member` of the attribute at `path` of `entity`: an attribute name, then the names of the sub-attributes
/// below it. Each level must hold one instance.
///
/// # Errors
///
/// Returns [`SystemTimeError`] when a level is missing or holds several instances, or the member is not written.
pub fn of_attribute<'e>(entity: &'e Entity<Output>, path: &[&str], member: SystemMember) -> Result<&'e DateTime, SystemTimeError> {
    let attribute = path.iter().try_fold(None::<&Attribute<Output>>, |parent, name| {
        let attributes = parent.map_or(&entity.attributes, |attribute| &attribute.meta().attributes);
        single(attributes, name).map(Some)
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
    .ok_or_else(|| SystemTimeError::Missing(path.join("."), member))
}

/// The one instance of the attribute `name` of `attributes`.
fn single<'a>(attributes: &'a Attributes<Output>, name: &str) -> Result<&'a Attribute<Output>, SystemTimeError> {
    let instances = attributes
        .iter()
        .find_map(|(candidate, instances)| (candidate.to_string() == name).then_some(instances))
        .ok_or_else(|| SystemTimeError::NoAttribute(name.to_owned()))?;
    if instances.len() == 1 {
        Ok(instances.first())
    } else {
        Err(SystemTimeError::Instances(name.to_owned(), instances.len()))
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
