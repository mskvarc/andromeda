//! The system times a broker writes on the instances of a temporal evolution (`createdAt`, `modifiedAt`,
//! `deletedAt`, clause 4.8), read from an answer so a later request can name the instant it answered.

use crate::{
    evolution_matcher::{InstanceChoice, instances},
    system_time::SystemMember,
};
use googletest::matcher::Matcher;
use ngsi_ld::model::{Attribute, AttributeName, DateTime, Output, evolution::TemporalEntity};
use std::fmt::Debug;

/// The evolution holds no instance the test names, or the instance carries no such time.
#[derive(Debug, thiserror::Error)]
#[error("the evolution of {name} holds no instance {which} carrying {member}")]
pub struct InstanceTimeNotFound<M: Debug> {
    /// The attribute name.
    name: AttributeName,
    /// Which instance was looked for.
    which: InstanceChoice<M>,
    /// The system member read.
    member: SystemMember,
}

/// The `member` of `instance`.
const fn time_of(instance: &Attribute<Output>, member: SystemMember) -> Option<DateTime> {
    let output = &instance.meta().output;
    match member {
        SystemMember::CreatedAt => output.created_at,
        SystemMember::ModifiedAt => output.modified_at,
        SystemMember::DeletedAt => output.deleted_at,
    }
}

/// The `member` of the first instance of the evolution `name` meeting `matcher`.
///
/// # Errors
///
/// Returns [`InstanceTimeNotFound`] when no instance meets it or the one that does carries no such time.
pub fn instance_time_where<M: for<'a> Matcher<&'a Attribute<Output>> + Debug>(
    entity: &TemporalEntity<Output>,
    name: &AttributeName,
    matcher: M,
    member: SystemMember,
) -> Result<DateTime, InstanceTimeNotFound<M>> {
    instances(entity, name.as_ref())
        .and_then(|found| found.iter().find(|instance| matcher.matches(instance).is_match()))
        .and_then(|instance| time_of(instance, member))
        .ok_or_else(|| InstanceTimeNotFound {
            // clone: the error names the attribute the caller keeps
            name: name.clone(),
            which: InstanceChoice::Meeting(matcher),
            member,
        })
}

/// The `member` of the instance at `index` (from 0) of the evolution `name`.
///
/// # Errors
///
/// Returns [`InstanceTimeNotFound`] when there is no such instance or it carries no such time.
pub fn instance_time_at(
    entity: &TemporalEntity<Output>,
    name: &AttributeName,
    index: usize,
    member: SystemMember,
) -> Result<DateTime, InstanceTimeNotFound<()>> {
    instances(entity, name.as_ref())
        .and_then(|found| found.get(index))
        .and_then(|instance| time_of(instance, member))
        .ok_or_else(|| InstanceTimeNotFound {
            // clone: the error names the attribute the caller keeps
            name: name.clone(),
            which: InstanceChoice::At(index),
            member,
        })
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_matcher::any_attribute,
        instance_time::{instance_time_at, instance_time_where},
        system_time::SystemMember,
    };
    use googletest::prelude::*;
    use ngsi_ld::model::{DateTime, Lossless, Output, evolution::TemporalEntity};

    /// A speed evolution of two instances, each with its creation time, as a broker answers it with `sysAttrs`.
    fn evolution() -> Result<TemporalEntity<Output>> {
        Ok(TemporalEntity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","speed":[
            {"type":"Property","value":1,"observedAt":"2024-01-01T00:00:00Z","createdAt":"2026-01-01T00:00:01Z"},
            {"type":"Property","value":2,"observedAt":"2024-01-01T00:01:00Z","createdAt":"2026-01-01T00:00:02.5Z"}]}"#,
            Lossless::Normalized,
        )?)
    }

    #[gtest]
    fn an_instance_picked_by_its_value_or_its_position_answers_its_time() -> Result<()> {
        let entity = evolution()?;
        expect_that!(
            instance_time_where(&entity, &"speed".parse()?, any_attribute().value(2), SystemMember::CreatedAt)?,
            eq("2026-01-01T00:00:02.5Z".parse::<DateTime>()?)
        );
        verify_that!(
            instance_time_at(&entity, &"speed".parse()?, 0, SystemMember::CreatedAt)?,
            eq("2026-01-01T00:00:01Z".parse::<DateTime>()?)
        )
    }

    #[gtest]
    fn a_missing_instance_or_time_is_an_error() -> Result<()> {
        let entity = evolution()?;
        expect_that!(
            instance_time_where(&entity, &"speed".parse()?, any_attribute().value(3), SystemMember::CreatedAt),
            err(anything())
        );
        verify_that!(instance_time_at(&entity, &"speed".parse()?, 0, SystemMember::ModifiedAt), err(anything()))
    }
}
