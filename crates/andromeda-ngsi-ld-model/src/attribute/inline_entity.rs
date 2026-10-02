//! The output-only inline entity payload carried by relationship members.

use crate::{attribute::one_or_many::OneOrMany, entity::Entity};

/// Output-only inline entity payload for `Relationship.entity`.
pub type InlineEntity = OneOrMany<Entity>;
