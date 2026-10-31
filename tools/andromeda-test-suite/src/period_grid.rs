//! The history of the page-walking aggregation tests: an `Agg` entity named `grid` whose `speed` instance `i` holds
//! the value `i` and was observed at the `i`th instant given, so each aggregation period of a grid the instants lie
//! on sums to its own index.

use crate::namespace::{Namespace, NamespaceError};
use ngsi_ld::model::{BuildError, Input, Property, TemporalError, evolution::TemporalEntity};

/// Why a history could not be built.
#[derive(Debug, thiserror::Error)]
pub enum PeriodGridError {
    /// A name or identifier is invalid.
    #[error(transparent)]
    Namespace(#[from] NamespaceError),
    /// An instant is invalid.
    #[error(transparent)]
    Instant(#[from] TemporalError),
    /// A member of the history is invalid.
    #[error(transparent)]
    Build(#[from] BuildError),
}

/// The entity `urn:ngsi-ld:Agg:<token>-grid` with one `speed` instance per instant of `observed`, the value counting
/// from 0.
///
/// # Errors
///
/// Returns [`PeriodGridError`] when a name, an instant or a member is invalid.
pub fn series(namespace: &Namespace, observed: &[&str]) -> Result<TemporalEntity<Input>, PeriodGridError> {
    let start = TemporalEntity::builder()
        .id(namespace.urn("Agg", "grid")?)
        .entity_type(namespace.entity_type("Agg")?);
    let with_speed = observed
        .iter()
        .zip(0_i32..)
        .try_fold(start, |builder, (instant, value)| -> Result<_, PeriodGridError> {
            Ok(builder.property("speed", Property::builder(value).observed_at(instant.parse()?)))
        })?;
    Ok(with_speed.build()?)
}

#[cfg(test)]
mod tests {
    use crate::{namespace::Namespace, period_grid::series};
    use googletest::prelude::*;

    #[gtest]
    fn each_instant_holds_its_index() -> Result<()> {
        let namespace = Namespace::fresh();
        let grid = series(&namespace, &["2020-01-01T00:00:00Z", "2020-02-01T00:00:00Z"])?;
        expect_that!(grid.id.to_string(), ends_with("-grid"));
        verify_that!(grid.attributes.len(), eq(1))
    }

    #[gtest]
    fn an_instant_that_is_not_one_is_refused() -> Result<()> {
        let namespace = Namespace::fresh();
        verify_that!(series(&namespace, &["yesterday"]), err(anything()))
    }
}
