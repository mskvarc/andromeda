//! The output-only inline entity payload carried by relationship members.

use crate::{attribute::one_or_many::OneOrMany, entity::Entity};
use jsonld::Expandable;
use jsonld_expandable_core::{JsonValue, ToJsonValue};

/// Output-only inline entity payload for `Relationship.entity`.
pub type InlineEntity = OneOrMany<Entity>;

impl Expandable for InlineEntity {
    fn expand<V: JsonValue>(&self) -> V {
        match self {
            OneOrMany::One(entity) => Expandable::expand::<V>(entity),
            OneOrMany::Many(entities) => V::array(entities.iter().map(Expandable::expand::<V>)),
        }
    }
}

impl<V: JsonValue> ToJsonValue<V> for InlineEntity {
    fn to_json_value(&self) -> V {
        Expandable::expand::<V>(self)
    }
}
