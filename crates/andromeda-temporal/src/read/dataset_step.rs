//! Whether a temporal read carries the `datasetId` step.
//!
//! Clauses 5.7.3.4 and 5.7.4.4 apply the `datasetId` parameter after the step
//! that decides which Entities answer, to "filter the Attribute instances" and
//! remove every Attribute left with none. A selected Entity can therefore read
//! back no instance at all, and still answer: both endpoints ask this before
//! treating an empty read as an Entity the window never selected.

use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;

/// Whether a temporal read narrows its instances by `datasetId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DatasetStep {
    /// No `datasetId` is named: every instance of the window is read.
    Absent,
    /// `datasetId`s are named: only their instances are read.
    Present,
}

impl DatasetStep {
    /// The step the compiled read carries.
    pub(crate) const fn of(compiled: &CompiledTemporalQuery) -> DatasetStep {
        if compiled.instances.dataset_id_filter.is_empty() {
            DatasetStep::Absent
        } else {
            DatasetStep::Present
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::read::dataset_step::DatasetStep;
    use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        dataset::DatasetSelector,
        entity_selection::EntitySelection,
        temporal::{compiled::CompiledTemporalQuery, instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use pretty_assertions::assert_eq;

    fn compiled(selectors: &[&str]) -> CompiledTemporalQuery {
        let mut compiled = CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).build())
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::Normalized)
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build();
        compiled.instances.dataset_id_filter = selectors
            .iter()
            .map(|selector| DatasetSelector::parse(selector).expect("a valid dataset selector"))
            .collect();
        compiled
    }

    #[test]
    fn a_read_naming_no_dataset_has_no_dataset_step() {
        assert_eq!(DatasetStep::of(&compiled(&[])), DatasetStep::Absent);
    }

    /// `@none` names the default instance (clause 4.5.5), so it is a dataset
    /// step like any named dataset.
    #[test]
    fn a_read_naming_the_default_or_a_named_dataset_has_the_step() {
        for selectors in [&["@none"][..], &["urn:ngsi-ld:Dataset:one"], &["@none", "urn:ngsi-ld:Dataset:one"]] {
            assert_eq!(DatasetStep::of(&compiled(selectors)), DatasetStep::Present, "{selectors:?}");
        }
    }
}
