//! Entities cut and combined by attribute name, and compared with what a broker answered.
//!
//! The ETSI cases state what a broker returns as "the entity of this file, without these attributes, with those
//! attributes as the other broker returned them". An answer is read back into the model's input form, so an expected
//! entity and an answer compare on their identifier, type and attributes as typed values.

use googletest::prelude::*;
use ngsi_ld::model::{AttributeName, Attributes, Entity, EntityId, Input, Output};

/// The input form of an entity a broker answered.
pub(crate) fn read_back(answer: Entity<Output>) -> Result<Entity<Input>> {
    Ok(answer.into_input()?)
}

/// The names `names` as attribute names.
pub(crate) fn attribute_names(names: &[&str]) -> Result<Vec<AttributeName>> {
    names.iter().map(|name| Ok(name.parse::<AttributeName>()?)).collect()
}

/// `entity` with only the attributes `names`.
pub(crate) fn keeping(entity: &Entity<Input>, names: &[&str]) -> Result<Entity<Input>> {
    let kept = attribute_names(names)?;
    let mut cut = entity.clone();
    let dropped: Vec<AttributeName> = cut.attributes.names().filter(|name| !kept.contains(name)).cloned().collect();
    for name in &dropped {
        cut.attributes.remove(name);
    }
    Ok(cut)
}

/// `entity` without the attributes `names`.
pub(crate) fn without(entity: &Entity<Input>, names: &[&str]) -> Result<Entity<Input>> {
    let dropped = attribute_names(names)?;
    let mut cut = entity.clone();
    for name in &dropped {
        cut.attributes.remove(name);
    }
    Ok(cut)
}

/// The attributes `names` of the entity `source`, as one part of a combined entity.
pub(crate) struct Part<'a> {
    /// The entity the attributes are taken from.
    pub(crate) source: &'a Entity<Input>,
    /// The attributes taken.
    pub(crate) names: &'a [&'a str],
}

/// The attributes `names` of `source`.
pub(crate) const fn taking<'a>(source: &'a Entity<Input>, names: &'a [&'a str]) -> Part<'a> {
    Part { source, names }
}

/// An entity with the identifier and type of `base` and exactly the attributes the `parts` take, each as its source
/// holds it.
pub(crate) fn combining(base: &Entity<Input>, parts: &[Part<'_>]) -> Result<Entity<Input>> {
    let mut combined = base.clone();
    combined.attributes = Attributes::new();
    for part in parts {
        for name in attribute_names(part.names)? {
            let instances = part.source.attributes.get(&name).or_fail()?;
            combined.attributes.set(name, instances.clone())?;
        }
    }
    Ok(combined)
}

/// Requires `actual` to have the identifier, type and attributes of `expected`.
pub(crate) fn expect_same(actual: &Entity<Input>, expected: &Entity<Input>) -> Result<()> {
    verify_that!(&actual.id, eq(&expected.id))?;
    verify_that!(&actual.entity_type, eq(&expected.entity_type))?;
    verify_that!(&actual.attributes, eq(&expected.attributes))
}

/// Requires the entities `found` to have exactly the identifiers `ids`, in any order.
pub(crate) fn expect_ids(found: &[Entity<Input>], ids: &[EntityId]) -> Result<()> {
    let mut found_ids: Vec<String> = found.iter().map(|entity| entity.id.to_string()).collect();
    found_ids.sort();
    let mut wanted: Vec<String> = ids.iter().map(ToString::to_string).collect();
    wanted.sort();
    verify_that!(found_ids, eq(&wanted))
}

/// The entity `id` among `found`, required to be `expected`.
pub(crate) fn expect_among<'a>(found: &'a [Entity<Input>], id: &EntityId, expected: &Entity<Input>) -> Result<&'a Entity<Input>> {
    let entity = found.iter().find(|entity| &entity.id == id).or_fail()?;
    expect_same(entity, expected)?;
    Ok(entity)
}

#[cfg(test)]
mod tests {
    use crate::{
        composition::{combining, expect_same, keeping, taking, without},
        parking::Sample,
    };
    use andromeda_test_suite::namespace::Namespace;
    use googletest::prelude::*;
    use ngsi_ld::model::{AttributeName, Entity, Input};

    fn names(entity: &Entity<Input>) -> Vec<String> {
        entity.attributes.names().map(ToString::to_string).collect()
    }

    #[gtest]
    fn keeping_leaves_only_the_named_attributes() -> Result<()> {
        let full = Sample::Parking1Full.entity(&Namespace::fresh())?;
        let counts = keeping(&full, &["availableSpotsNumber", "totalSpotsNumber"])?;
        verify_that!(names(&counts), unordered_elements_are![eq("availableSpotsNumber"), eq("totalSpotsNumber")])
    }

    #[gtest]
    fn without_drops_the_named_attributes() -> Result<()> {
        let full = Sample::Parking1Full.entity(&Namespace::fresh())?;
        let cut = without(&full, &["location"])?;
        verify_that!(
            names(&cut),
            unordered_elements_are![eq("name"), eq("availableSpotsNumber"), eq("totalSpotsNumber")]
        )
    }

    #[gtest]
    fn combining_takes_each_attribute_from_its_source() -> Result<()> {
        let namespace = Namespace::fresh();
        let counts = Sample::Parking1WithoutLocation.entity(&namespace)?;
        let located = Sample::Parking1Full.entity(&namespace)?;
        let combined = combining(
            &located,
            &[taking(&counts, &["availableSpotsNumber", "totalSpotsNumber"]), taking(&located, &["location"])],
        )?;
        let available: AttributeName = "availableSpotsNumber".parse()?;
        verify_that!(combined.attributes.get(&available), some(eq(counts.attributes.get(&available).or_fail()?)))?;
        verify_that!(
            names(&combined),
            unordered_elements_are![eq("availableSpotsNumber"), eq("totalSpotsNumber"), eq("location")]
        )
    }

    #[gtest]
    fn combining_refuses_an_attribute_its_source_lacks() -> Result<()> {
        let namespace = Namespace::fresh();
        let partial = Sample::Parking1LocationAndName.entity(&namespace)?;
        verify_that!(combining(&partial, &[taking(&partial, &["totalSpotsNumber"])]), err(anything()))
    }

    #[gtest]
    fn entities_that_differ_in_one_value_are_not_the_same() -> Result<()> {
        let namespace = Namespace::fresh();
        let first = Sample::Parking1Full.entity(&namespace)?;
        let second = Sample::Parking1WithoutLocation.entity(&namespace)?;
        verify_that!(expect_same(&first, &second), err(anything()))
    }
}
