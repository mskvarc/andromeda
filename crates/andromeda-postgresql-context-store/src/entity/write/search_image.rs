//! The images of an `entity_search` row a set-write statement hands on.
//!
//! Every statement that writes `entity_search` returns what it wrote, and the
//! statements that follow it in the same `WITH` read that `RETURNING` instead
//! of the relation. That is not a style: PostgreSQL runs every sub-statement of
//! one `WITH` on the snapshot the statement started with, and the `RETURNING`
//! rows are the only way one sub-statement sees what another wrote. A sibling
//! that joined `entity_search` itself would read the row as the snapshot saw
//! it, not as the write found it after waiting on a concurrent writer's lock,
//! so a type membership or a census count derived from it would drift from the
//! row actually stored. The images here are named by side: `new_*` is the row
//! the statement left (an insert, or an update's post-image), `old_*` the row it
//! replaced or removed (an update's pre-image, or a delete).

use sea_query::Iden;

/// A column of the images a write of `entity_search` returns.
#[derive(Iden, Clone, Copy)]
pub(crate) enum SearchImage {
    /// The type keys the row holds after the write.
    #[iden = "new_types"]
    NewTypes,
    /// The attribute instance name keys the row holds after the write.
    #[iden = "new_instance_names"]
    NewInstanceNames,
    /// The attribute instance family codes the row holds after the write.
    #[iden = "new_instance_kinds"]
    NewInstanceKinds,
    /// The type keys the row held before the write.
    #[iden = "old_types"]
    OldTypes,
    /// The attribute instance name keys the row held before the write.
    #[iden = "old_instance_names"]
    OldInstanceNames,
    /// The attribute instance family codes the row held before the write.
    #[iden = "old_instance_kinds"]
    OldInstanceKinds,
}

/// One side of a returned `entity_search` image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageSide {
    /// The row as the write left it.
    New,
    /// The row as it stood before the write.
    Old,
}

impl ImageSide {
    /// The image column holding this side's type keys.
    pub(crate) const fn types(self) -> SearchImage {
        match self {
            ImageSide::New => SearchImage::NewTypes,
            ImageSide::Old => SearchImage::OldTypes,
        }
    }

    /// The image column holding this side's attribute instance name keys.
    pub(crate) const fn instance_names(self) -> SearchImage {
        match self {
            ImageSide::New => SearchImage::NewInstanceNames,
            ImageSide::Old => SearchImage::OldInstanceNames,
        }
    }

    /// The image column holding this side's attribute instance family codes.
    pub(crate) const fn instance_kinds(self) -> SearchImage {
        match self {
            ImageSide::New => SearchImage::NewInstanceKinds,
            ImageSide::Old => SearchImage::OldInstanceKinds,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::write::search_image::{ImageSide, SearchImage};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn each_side_names_its_own_three_columns() {
        let names = |side: ImageSide| [side.types().to_string(), side.instance_names().to_string(), side.instance_kinds().to_string()];
        assert_eq!(names(ImageSide::New), ["new_types", "new_instance_names", "new_instance_kinds"]);
        assert_eq!(names(ImageSide::Old), ["old_types", "old_instance_names", "old_instance_kinds"]);
        assert_eq!(SearchImage::OldTypes.to_string(), "old_types");
    }
}
