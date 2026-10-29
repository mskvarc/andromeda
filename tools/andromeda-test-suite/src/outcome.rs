//! The whole success of a typed call: an outcome that may also be partial, required to be complete.
//!
//! Create Entity, Replace Entity, the attribute updates and the batch operations succeed either wholly (201 or 204)
//! or partly (207 Multi-Status, clause 6.3.3). A test that expects the whole success requires it with
//! [`Done::done`], so a partial answer fails the test with its report.

use ngsi_ld::{
    http::{BatchCreateOutcome, BatchOutcome, CreateOutcome, TemporalUpsertOutcome, UpdateOutcome, UpsertOutcome},
    model::EntityId,
};
use std::fmt::Debug;

/// The broker answered only a partial success, with the outcome reporting it.
#[derive(Debug, thiserror::Error)]
#[error("the broker answered a partial success: {0:?}")]
pub struct Partial<O: Debug>(O);

/// An outcome that is the whole success or a partial one.
pub trait Done: Debug + Sized {
    /// What the whole success carries.
    type Value;

    /// The whole success.
    ///
    /// # Errors
    ///
    /// Returns [`Partial`] with the broker's report when the success is partial.
    fn done(self) -> Result<Self::Value, Partial<Self>>;
}

impl Done for CreateOutcome {
    type Value = ();

    fn done(self) -> Result<(), Partial<Self>> {
        match self {
            CreateOutcome::Created { .. } => Ok(()),
            partial @ CreateOutcome::PartiallyCreated { .. } => Err(Partial(partial)),
        }
    }
}

impl Done for BatchOutcome {
    type Value = ();

    fn done(self) -> Result<(), Partial<Self>> {
        match self {
            BatchOutcome::Done => Ok(()),
            partial @ BatchOutcome::Partial(_) => Err(Partial(partial)),
        }
    }
}

impl Done for UpdateOutcome {
    type Value = ();

    fn done(self) -> Result<(), Partial<Self>> {
        match self {
            UpdateOutcome::Done => Ok(()),
            partial @ UpdateOutcome::Partial(_) => Err(Partial(partial)),
        }
    }
}

impl Done for BatchCreateOutcome {
    type Value = Vec<EntityId>;

    fn done(self) -> Result<Vec<EntityId>, Partial<Self>> {
        match self {
            BatchCreateOutcome::Created(ids) => Ok(ids),
            partial @ BatchCreateOutcome::Partial(_) => Err(Partial(partial)),
        }
    }
}

impl Done for UpsertOutcome {
    type Value = UpsertOutcome;

    fn done(self) -> Result<UpsertOutcome, Partial<Self>> {
        match self {
            partial @ UpsertOutcome::Partial(_) => Err(Partial(partial)),
            whole @ (UpsertOutcome::Created(_) | UpsertOutcome::Updated) => Ok(whole),
        }
    }
}

/// The broker answered another success than the test expects of an upsert: the outcome, and the success expected.
#[derive(Debug, thiserror::Error)]
#[error("the broker answered {0:?}, not {1}")]
pub struct OtherSuccess<O: Debug>(O, ExpectedSuccess);

/// The success a test expects of an upsert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum ExpectedSuccess {
    /// The resource was created.
    #[strum(serialize = "201 Created")]
    Created,
    /// The resource was updated.
    #[strum(serialize = "204 No Content")]
    NoContent,
}

/// An upsert's outcome: the resource was created (201) or updated (204).
pub trait Upserted: Debug + Sized {
    /// Requires the 201 Created outcome.
    ///
    /// # Errors
    ///
    /// Returns [`OtherSuccess`] for any other outcome.
    fn created(self) -> Result<(), OtherSuccess<Self>>;

    /// Requires the 204 No Content outcome.
    ///
    /// # Errors
    ///
    /// Returns [`OtherSuccess`] for any other outcome.
    fn updated(self) -> Result<(), OtherSuccess<Self>>;
}

impl Upserted for TemporalUpsertOutcome {
    fn created(self) -> Result<(), OtherSuccess<Self>> {
        match self {
            TemporalUpsertOutcome::Created { .. } => Ok(()),
            other @ TemporalUpsertOutcome::Updated => Err(OtherSuccess(other, ExpectedSuccess::Created)),
        }
    }

    fn updated(self) -> Result<(), OtherSuccess<Self>> {
        match self {
            TemporalUpsertOutcome::Updated => Ok(()),
            other @ TemporalUpsertOutcome::Created { .. } => Err(OtherSuccess(other, ExpectedSuccess::NoContent)),
        }
    }
}

/// A Batch Upsert's outcome read for the entities it created.
pub trait UpsertCreated: Debug + Sized {
    /// The ids of the entities the upsert created, which its 201 answer lists (clause 6.15.3.1).
    ///
    /// # Errors
    ///
    /// Returns [`OtherSuccess`] for the 204 answer of an upsert that created nothing, or a partial success.
    fn created_entities(self) -> Result<Vec<EntityId>, OtherSuccess<Self>>;
}

impl UpsertCreated for UpsertOutcome {
    fn created_entities(self) -> Result<Vec<EntityId>, OtherSuccess<Self>> {
        match self {
            UpsertOutcome::Created(ids) => Ok(ids),
            other @ (UpsertOutcome::Updated | UpsertOutcome::Partial(_)) => Err(OtherSuccess(other, ExpectedSuccess::Created)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::outcome::{Done, UpsertCreated};
    use googletest::prelude::*;
    use iri_rs::iri;
    use ngsi_ld::{
        http::{BatchOutcome, UpdateOutcome, UpsertOutcome},
        model::EntityId,
    };

    #[gtest]
    fn a_whole_success_is_done() -> Result<()> {
        expect_that!(BatchOutcome::Done.done(), ok(eq(&())));
        verify_that!(UpdateOutcome::Done.done(), ok(eq(&())))
    }

    #[gtest]
    fn an_upsert_that_created_lists_its_entities_and_one_that_updated_fails() -> Result<()> {
        let created: EntityId = iri!("urn:ngsi-ld:Vehicle:1").into();
        expect_that!(
            UpsertOutcome::Created(vec![created.clone()]).created_entities(),
            ok(elements_are![eq(&created)])
        );
        verify_that!(UpsertOutcome::Updated.created_entities(), err(anything()))
    }
}
