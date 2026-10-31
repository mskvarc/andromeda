//! The evolutions a temporal entity answered under a `pick` or `omit` holds (clause 4.21): the projected entity keeps
//! only the members the projection leaves, so a test names the attributes that are there and those that are not.

use ngsi_ld::model::{Completeness, Output, evolution::TemporalEntity};

/// The names of the attributes `entity` holds an evolution of, in the answer's order.
#[must_use]
pub fn evolution_names<C: Completeness>(entity: &TemporalEntity<Output, C>) -> Vec<String> {
    entity.attributes.names().map(ToString::to_string).collect()
}

#[cfg(test)]
mod tests {
    use crate::projected_evolution::evolution_names;
    use googletest::prelude::*;
    use ngsi_ld::model::{Lossless, Output, evolution::TemporalEntity};

    #[gtest]
    fn the_attributes_of_an_answer_are_named_in_order() -> Result<()> {
        let entity = TemporalEntity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:Building:1","type":"Building",
            "category":[{"type":"Property","value":"commercial","observedAt":"2020-08-01T12:03:00Z"}],
            "name":[{"type":"Property","value":"Depot","observedAt":"2020-08-01T12:03:00Z"}]}"#,
            Lossless::Normalized,
        )?;
        verify_that!(evolution_names(&entity), elements_are![eq("category"), eq("name")])
    }
}
