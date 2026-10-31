//! Reads of an entity type information document (clause 5.2.26) by attribute: the families the type lists for one
//! attribute and the attributes it lists at all, so a test compares them without walking `attributeDetails` itself.

use ngsi_ld::model::{AttributeKind, AttributeName, discovery::EntityTypeInfo};

/// The families the information lists for `name`, in the order the broker wrote them; `None` when the type lists no
/// such attribute or the attribute carries no `attributeTypes` member.
#[must_use]
pub fn families_of<'a>(information: &'a EntityTypeInfo, name: &AttributeName) -> Option<&'a [AttributeKind]> {
    information
        .attribute_details
        .iter()
        .find(|detail| detail.attribute_name == *name)
        .and_then(|detail| detail.attribute_types.as_deref())
}

/// The names of the attributes the information lists, in the order the broker wrote them.
#[must_use]
pub fn attribute_names(information: &EntityTypeInfo) -> Vec<AttributeName> {
    information.attribute_details.iter().map(|detail| detail.attribute_name.clone()).collect()
}

#[cfg(test)]
mod tests {
    use crate::type_information::{attribute_names, families_of};
    use googletest::prelude::*;
    use ngsi_ld::model::{AttributeKind, AttributeName, discovery::EntityTypeInfo};

    /// The information of a type with a Property and a `GeoProperty` attribute.
    fn information() -> Result<EntityTypeInfo> {
        Ok(EntityTypeInfo::parse(concat!(
            r#"{"id":"https://example.org/Vehicle","type":"EntityTypeInfo","attributeDetails":["#,
            r#"{"id":"https://example.org/speed","type":"Attribute","attributeName":"speed","attributeTypes":["Property"]},"#,
            r#"{"id":"https://example.org/location","type":"Attribute","attributeName":"location","attributeTypes":["GeoProperty"]}],"#,
            r#""entityCount":2,"typeName":"Vehicle"}"#
        ))?)
    }

    #[gtest]
    fn the_families_of_a_listed_attribute_are_its_attribute_types() -> Result<()> {
        let speed: AttributeName = "speed".parse()?;
        verify_that!(families_of(&information()?, &speed), some(elements_are![eq(&AttributeKind::Property)]))
    }

    #[gtest]
    fn an_attribute_the_type_does_not_list_has_no_families() -> Result<()> {
        let missing: AttributeName = "missing".parse()?;
        verify_that!(families_of(&information()?, &missing), none())
    }

    #[gtest]
    fn the_listed_names_follow_the_order_of_the_details() -> Result<()> {
        let speed: AttributeName = "speed".parse()?;
        let location: AttributeName = "location".parse()?;
        verify_that!(attribute_names(&information()?), elements_are![eq(&speed), eq(&location)])
    }
}
