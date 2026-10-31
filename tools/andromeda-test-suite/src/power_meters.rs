//! The six `Meter` entities of the temporal `q` window tests, whose `power` histories differ in which instances are in
//! effect when a window opens, and the `status` instance appended to each afterwards.
//!
//! The instants are taken from the runner's clock: `t30` a quarter of an hour ago, then 5, 15, 30, 45 and 75 minutes
//! ahead.

use crate::{
    broker::BrokerClient,
    instant_ahead::minutes_ahead,
    namespace::{Namespace, NamespaceError},
};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    model::{
        Attribute,
        AttributeName,
        AttributesError,
        BuildError,
        DateTime,
        Entity,
        EntityId,
        EntityTypeName,
        Input,
        NameError,
        Property,
        TemporalError,
        evolution::{TemporalAttributes, TemporalEntity},
    },
};

/// Why the meters could not be written.
#[derive(Debug, thiserror::Error)]
pub enum PowerMetersError {
    /// A name or identifier is invalid.
    #[error(transparent)]
    Namespace(#[from] NamespaceError),
    /// An instant is invalid.
    #[error(transparent)]
    Instant(#[from] TemporalError),
    /// A member of an entity is invalid.
    #[error(transparent)]
    Build(#[from] BuildError),
    /// An attribute name is invalid.
    #[error(transparent)]
    Name(#[from] NameError),
    /// A fragment could not take its attribute.
    #[error("the status fragment could not be built")]
    Fragment(#[source] AttributesError),
    /// The broker refused a write.
    #[error("the broker refused a write")]
    Client(#[source] Box<ClientError<ReqwestError>>),
}

/// The instants of the window tests, named by the minutes past the start of the history they stand for.
#[derive(Debug, Clone, Copy)]
pub struct Instants {
    /// A quarter of an hour ago.
    pub t30: DateTime,
    /// Five minutes ahead.
    pub t50: DateTime,
    /// A quarter of an hour ahead.
    pub t60: DateTime,
    /// Half an hour ahead.
    pub t75: DateTime,
    /// Three quarters of an hour ahead.
    pub t90: DateTime,
    /// An hour and a quarter ahead.
    pub t120: DateTime,
}

/// The instants, from the runner's clock.
///
/// # Errors
///
/// Returns [`TemporalError`] when the clock's rendering is not an instant, which it never is.
pub fn instants() -> Result<Instants, TemporalError> {
    Ok(Instants {
        t30: minutes_ahead(-15).parse()?,
        t50: minutes_ahead(5).parse()?,
        t60: minutes_ahead(15).parse()?,
        t75: minutes_ahead(30).parse()?,
        t90: minutes_ahead(45).parse()?,
        t120: minutes_ahead(75).parse()?,
    })
}

/// The identifier of the meter `suffix` (`M1` to `M6`).
///
/// # Errors
///
/// Returns [`NamespaceError`] when the identifier is invalid.
pub fn meter_id(namespace: &Namespace, suffix: &str) -> Result<EntityId, NamespaceError> {
    namespace.urn("Meter", suffix)
}

/// A meter of `meter_type` named `suffix` whose `power` instances are `(value, observed, dataset)`.
fn history(
    namespace: &Namespace,
    meter_type: &EntityTypeName,
    suffix: &str,
    instances: &[(i32, DateTime, Option<&str>)],
) -> Result<TemporalEntity<Input>, PowerMetersError> {
    let start = TemporalEntity::builder().id(meter_id(namespace, suffix)?).entity_type(meter_type.clone());
    let built = instances
        .iter()
        .try_fold(start, |builder, (value, observed, dataset)| -> Result<_, PowerMetersError> {
            let property = Property::builder(*value).observed_at(*observed);
            Ok(match dataset {
                Some(name) => builder.property("power", property.dataset_id(namespace.dataset_id("Dataset", name)?)),
                None => builder.property("power", property),
            })
        })?;
    Ok(built.build()?)
}

/// Writes the six meters: M1, M2, M4, M5 and M6 as temporal evolutions, and M3 as an entity whose `power` is deleted.
///
/// # Errors
///
/// Returns [`PowerMetersError`] when an entity is invalid or the broker refuses a write.
pub async fn write_power(client: &BrokerClient, namespace: &Namespace, at: &Instants) -> Result<(), PowerMetersError> {
    let meter_type = namespace.entity_type("Meter")?;
    let evolutions = [
        history(namespace, &meter_type, "M1", &[(5, at.t30, None), (9, at.t90, None)])?,
        history(namespace, &meter_type, "M2", &[(9, at.t30, None)])?,
        history(namespace, &meter_type, "M4", &[(9, at.t30, None), (1, at.t60, None)])?,
        history(namespace, &meter_type, "M5", &[(1, at.t30, None), (9, at.t30, None)])?,
        history(namespace, &meter_type, "M6", &[(9, at.t30, Some("a")), (1, at.t50, None)])?,
    ];
    for evolution in &evolutions {
        client
            .temporal()
            .create_or_update(evolution)
            .send()
            .await
            .map_err(|error| PowerMetersError::Client(Box::new(error)))?;
    }
    let m3 = Entity::builder()
        .id(meter_id(namespace, "M3")?)
        .entity_type(meter_type)
        .property("power", Property::builder(9).observed_at(at.t30))
        .build()?;
    client
        .entities()
        .create(&m3)
        .send()
        .await
        .map_err(|error| PowerMetersError::Client(Box::new(error)))?;
    client
        .entities()
        .delete_attribute(&meter_id(namespace, "M3")?, &"power".parse::<AttributeName>()?)
        .send()
        .await
        .map_err(|error| PowerMetersError::Client(Box::new(error)))?;
    Ok(())
}

/// Appends a `status` instance `on` observed at `at.t75` to each of the six meters.
///
/// # Errors
///
/// Returns [`PowerMetersError`] when a fragment is invalid or the broker refuses a write.
pub async fn append_status(client: &BrokerClient, namespace: &Namespace, at: &Instants) -> Result<(), PowerMetersError> {
    for suffix in ["M1", "M2", "M3", "M4", "M5", "M6"] {
        let mut fragment = TemporalAttributes::<Input>::new();
        fragment
            .insert("status".parse()?, Attribute::from(Property::builder("on").observed_at(at.t75).build()?))
            .map_err(PowerMetersError::Fragment)?;
        client
            .temporal()
            .add_attributes(&meter_id(namespace, suffix)?, &fragment)
            .send()
            .await
            .map_err(|error| PowerMetersError::Client(Box::new(error)))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        namespace::Namespace,
        power_meters::{history, instants, meter_id},
    };
    use googletest::prelude::*;

    #[gtest]
    fn the_instants_rise_from_the_past_to_the_future() -> Result<()> {
        let at = instants()?;
        verify_that!(
            [at.t30, at.t50, at.t60, at.t75, at.t90, at.t120]
                .windows(2)
                .all(|pair| matches!(pair, [earlier, later] if earlier < later)),
            eq(true)
        )
    }

    #[gtest]
    fn a_history_holds_one_power_instance_per_entry() -> Result<()> {
        let namespace = Namespace::fresh();
        let at = instants()?;
        let evolution = history(&namespace, &namespace.entity_type("Meter")?, "M6", &[(9, at.t30, Some("a")), (1, at.t50, None)])?;
        expect_that!(evolution.id, eq(&meter_id(&namespace, "M6")?));
        verify_that!(evolution.attributes.len(), eq(1))
    }
}
