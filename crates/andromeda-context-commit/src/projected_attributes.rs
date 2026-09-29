//! The attributes an entity payload carries, with the family of every instance
//! of each.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use std::iter::once;

/// The top-level members of an expanded payload that clause 5.5.9 `attrs`
/// reads as attribute names, each with the families of the attribute instances
/// it holds: one per `datasetId` (clause 4.5.5), which is what clause 5.7
/// discovery counts. A member NGSI-LD reserves (`scope`, the Entity's own
/// `expiresAt`) is named here, as `attrs` reads it, and holds no instance.
///
/// The families of every member live in one vector, with where each member's
/// run ends, so projecting a batch of entities allocates no vector per
/// attribute; the runs cannot fall out of step with the names because
/// [`ProjectedAttributes::push`] is the only way to add either.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProjectedAttributes {
    /// The member names, in payload order.
    names: Vec<AttributeName>,
    /// Where each member's run of families ends in `families`, aligned with
    /// `names`.
    ends: Vec<usize>,
    /// The family of every instance, member by member.
    families: Vec<AttributeType>,
}

impl ProjectedAttributes {
    /// Add one member and the families `instances` reads for it.
    pub fn push<I>(&mut self, name: AttributeName, instances: impl FnOnce(&AttributeName) -> I)
    where
        I: IntoIterator<Item = AttributeType>,
    {
        self.families.extend(instances(&name));
        self.ends.push(self.families.len());
        self.names.push(name);
    }

    /// The member names, in payload order.
    #[must_use]
    pub fn names(&self) -> &[AttributeName] {
        &self.names
    }

    /// Every member with the families of its instances, in payload order.
    pub fn members(&self) -> impl Iterator<Item = (&AttributeName, &[AttributeType])> {
        let starts = once(0).chain(self.ends.iter().copied());
        self.names
            .iter()
            .zip(starts.zip(self.ends.iter().copied()))
            .map(|(name, (start, end))| (name, &self.families[start..end]))
    }

    /// How many attribute instances the members hold together.
    #[must_use]
    pub const fn instance_count(&self) -> usize {
        self.families.len()
    }
}

#[cfg(test)]
mod tests {
    use crate::projected_attributes::ProjectedAttributes;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
    use pretty_assertions::assert_eq;

    fn name(raw: &str) -> AttributeName {
        raw.parse().expect("a valid attribute name")
    }

    #[test]
    fn each_member_keeps_the_run_of_families_it_was_pushed_with() {
        let mut attributes = ProjectedAttributes::default();
        attributes.push(name("https://example.org/a"), |_| [AttributeType::Property, AttributeType::Relationship]);
        attributes.push(name("https://example.uri/scope"), |_| []);
        attributes.push(name("https://example.org/b"), |_| [AttributeType::GeoProperty]);
        let members: Vec<(&str, Vec<AttributeType>)> = attributes.members().map(|(name, families)| (name.as_str(), families.to_vec())).collect();
        assert_eq!(
            members,
            vec![
                ("https://example.org/a", vec![AttributeType::Property, AttributeType::Relationship]),
                ("https://example.uri/scope", Vec::new()),
                ("https://example.org/b", vec![AttributeType::GeoProperty]),
            ]
        );
        assert_eq!(attributes.names().len(), 3);
        assert_eq!(attributes.instance_count(), 3);
    }

    #[test]
    fn no_members_hold_no_instances() {
        let attributes = ProjectedAttributes::default();
        assert_eq!(attributes.members().count(), 0);
        assert_eq!(attributes.instance_count(), 0);
    }
}
