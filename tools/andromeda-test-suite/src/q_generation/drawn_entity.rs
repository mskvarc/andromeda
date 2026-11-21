//! One drawn Entity: the Attributes it is created with, those its second write appends, and what it holds after each.

use crate::q_generation::drawn_attribute::DrawnAttribute;
use strum::EnumIter;

/// The write after which a drawn Entity is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter)]
pub enum DrawnState {
    /// It has been created.
    AfterCreation,
    /// Its second write has appended its change.
    AfterChange,
}

/// One drawn Entity and the change its second write makes.
#[derive(Debug, Clone)]
pub struct DrawnEntity {
    /// The Attributes it is created with, each name once.
    pub attributes: Vec<DrawnAttribute>,
    /// The Attributes the second write appends, each name once.
    pub change: Vec<DrawnAttribute>,
}

impl DrawnEntity {
    /// The Attributes it holds in `state`. After its change, an appended Attribute replaces the one of its name, and
    /// follows those the change leaves in place.
    #[must_use]
    pub fn held(&self, state: DrawnState) -> Vec<&DrawnAttribute> {
        match state {
            DrawnState::AfterCreation => self.attributes.iter().collect(),
            DrawnState::AfterChange => self
                .attributes
                .iter()
                .filter(|attribute| self.change.iter().all(|appended| appended.name != attribute.name))
                .chain(&self.change)
                .collect(),
        }
    }
}
