//! The lists of a stored notification told apart as expanded or written.

use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName};
use andromeda_ngsi_ld_model::{staged_list::StagedList, subscription::unreadable_member::UnreadableMember};
use andromeda_ngsi_ld_query::projection::{expanded_projection_list::ExpandedProjectionList, written_projection_list::WrittenProjectionList};
use serde::Deserialize;

/// The `attributes`, `pick` and `omit` of the `notification` column, read as the one stage that holds them and nothing else of the column.
///
/// The notification parameters read a list that holds a name that is no absolute IRI as absent, so this reading is what keeps that list from widening what the subscription selects: it names each such list, and the subscription lists it as unreadable.
#[derive(Debug, Deserialize)]
pub(super) struct NotificationLists {
    /// The attributes the notification carries.
    attributes: Option<StagedList<AttributeIri, AttributeName>>,
    /// The projections that keep members.
    pick: Option<StagedList<ExpandedProjectionList, WrittenProjectionList>>,
    /// The projections that remove members.
    omit: Option<StagedList<ExpandedProjectionList, WrittenProjectionList>>,
}

impl NotificationLists {
    /// The members whose list holds a name that is no absolute IRI.
    pub(super) fn unreadable(&self) -> Vec<UnreadableMember> {
        [
            (
                matches!(self.attributes, Some(StagedList::Written(_))),
                UnreadableMember::NotificationAttributes,
            ),
            (matches!(self.pick, Some(StagedList::Written(_))), UnreadableMember::Pick),
            (matches!(self.omit, Some(StagedList::Written(_))), UnreadableMember::Omit),
        ]
        .into_iter()
        .filter_map(|(written, member)| written.then_some(member))
        .collect()
    }
}
