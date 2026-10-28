//! The whole success of a typed call: an outcome that may also be partial, required to be complete.
//!
//! Create Entity, Replace Entity, the attribute updates and the batch operations succeed either wholly (201 or 204)
//! or partly (207 Multi-Status, clause 6.3.3). A test that expects the whole success requires it with
//! [`Done::done`], so a partial answer fails the test with its report.

use ngsi_ld::{
    http::{BatchCreateOutcome, BatchOutcome, CreateOutcome, TemporalUpsertOutcome, UpdateOutcome, UpsertOutcome},
    model::EntityId,
};

/// The broker answered only a partial success.
#[derive(Debug, thiserror::Error)]
#[error("the broker answered a partial success: {0}")]
pub struct Partial(String);

/// An outcome that is the whole success or a partial one.
pub trait Done {
    /// What the whole success carries.
    type Value;

    /// The whole success.
    ///
    /// # Errors
    ///
    /// Returns [`Partial`] with the broker's report when the success is partial.
    fn done(self) -> Result<Self::Value, Partial>;
}

impl Done for CreateOutcome {
    type Value = ();

    fn done(self) -> Result<(), Partial> {
        match self {
            CreateOutcome::Created { .. } => Ok(()),
            partial @ CreateOutcome::PartiallyCreated { .. } => Err(Partial(format!("{partial:?}"))),
        }
    }
}

impl Done for BatchOutcome {
    type Value = ();

    fn done(self) -> Result<(), Partial> {
        match self {
            BatchOutcome::Done => Ok(()),
            BatchOutcome::Partial(result) => Err(Partial(format!("{result:?}"))),
        }
    }
}

impl Done for UpdateOutcome {
    type Value = ();

    fn done(self) -> Result<(), Partial> {
        match self {
            UpdateOutcome::Done => Ok(()),
            UpdateOutcome::Partial(result) => Err(Partial(format!("{result:?}"))),
        }
    }
}

impl Done for BatchCreateOutcome {
    type Value = Vec<EntityId>;

    fn done(self) -> Result<Vec<EntityId>, Partial> {
        match self {
            BatchCreateOutcome::Created(ids) => Ok(ids),
            BatchCreateOutcome::Partial(result) => Err(Partial(format!("{result:?}"))),
        }
    }
}

impl Done for UpsertOutcome {
    type Value = UpsertOutcome;

    fn done(self) -> Result<UpsertOutcome, Partial> {
        match self {
            UpsertOutcome::Partial(result) => Err(Partial(format!("{result:?}"))),
            whole @ (UpsertOutcome::Created(_) | UpsertOutcome::Updated) => Ok(whole),
        }
    }
}

/// The broker answered another success than the test expects of an upsert.
#[derive(Debug, thiserror::Error)]
#[error("the broker answered {0}, not {1}")]
pub struct OtherSuccess(String, &'static str);

/// An upsert's outcome: the resource was created (201) or updated (204).
pub trait Upserted {
    /// Requires the 201 Created outcome.
    ///
    /// # Errors
    ///
    /// Returns [`OtherSuccess`] for any other outcome.
    fn created(self) -> Result<(), OtherSuccess>;

    /// Requires the 204 No Content outcome.
    ///
    /// # Errors
    ///
    /// Returns [`OtherSuccess`] for any other outcome.
    fn updated(self) -> Result<(), OtherSuccess>;
}

impl Upserted for TemporalUpsertOutcome {
    fn created(self) -> Result<(), OtherSuccess> {
        match self {
            TemporalUpsertOutcome::Created { .. } => Ok(()),
            other @ TemporalUpsertOutcome::Updated => Err(OtherSuccess(format!("{other:?}"), "201 Created")),
        }
    }

    fn updated(self) -> Result<(), OtherSuccess> {
        match self {
            TemporalUpsertOutcome::Updated => Ok(()),
            other @ TemporalUpsertOutcome::Created { .. } => Err(OtherSuccess(format!("{other:?}"), "204 No Content")),
        }
    }
}

/// A Batch Upsert's outcome read for the entities it created.
pub trait UpsertCreated {
    /// The ids of the entities the upsert created, which its 201 answer lists (clause 6.15.3.1).
    ///
    /// # Errors
    ///
    /// Returns [`OtherSuccess`] for the 204 answer of an upsert that created nothing, or a partial success.
    fn created_entities(self) -> Result<Vec<EntityId>, OtherSuccess>;
}

impl UpsertCreated for UpsertOutcome {
    fn created_entities(self) -> Result<Vec<EntityId>, OtherSuccess> {
        match self {
            UpsertOutcome::Created(ids) => Ok(ids),
            other @ (UpsertOutcome::Updated | UpsertOutcome::Partial(_)) => Err(OtherSuccess(format!("{other:?}"), "201 Created")),
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
