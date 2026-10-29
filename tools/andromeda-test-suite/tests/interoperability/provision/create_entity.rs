//! Create Entity at b1 with registrations of every mode: the entity is created in b1 and forwarded to the context
//! sources whose registrations match it, each of which holds the attributes its registration covers.

mod iop_001_01_01;
mod iop_001_01_02;
mod iop_001_02_01;
mod iop_001_02_02;
mod iop_001_03_01;
mod iop_001_03_02;
mod iop_001_04_01;
mod iop_001_04_02;

use crate::{
    composition::{expect_same, keeping, without},
    node::Node,
    parking::Sample,
    scenario::Scenario,
};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Input};

/// What a broker holds of the created entity, read from the broker alone (`local=true`).
pub(crate) enum Holds {
    /// The entity as created.
    Whole,
    /// Only these attributes of the entity as created.
    Only(&'static [&'static str]),
    /// The entity as created without these attributes.
    Without(&'static [&'static str]),
    /// No entity.
    Nothing,
}

impl Holds {
    /// The entity the broker holds when `created` was sent, none when it holds nothing.
    fn expected(&self, created: &Entity<Input>) -> Result<Option<Entity<Input>>> {
        match self {
            Holds::Whole => Ok(Some(created.clone())), // clone: the case keeps the created entity for its other expectations
            Holds::Only(names) => Ok(Some(keeping(created, names)?)),
            Holds::Without(names) => Ok(Some(without(created, names)?)),
            Holds::Nothing => Ok(None),
        }
    }
}

/// Creates the entity of `sample` through b1 and requires each broker of `expectations` to hold what its
/// [`Holds`] says.
pub(crate) async fn create_at_b1(scenario: &mut Scenario, sample: Sample, expectations: &[(Node, Holds)]) -> Result<()> {
    let created = sample.entity(scenario.namespace())?;
    scenario.create(Node::B1, &created).await?;
    for (node, holds) in expectations {
        match holds.expected(&created)? {
            Some(expected) => expect_same(&scenario.read_local(*node, &created.id).await?, &expected)?,
            None => scenario.expect_absent(*node, &created.id).await?,
        }
    }
    Ok(())
}
