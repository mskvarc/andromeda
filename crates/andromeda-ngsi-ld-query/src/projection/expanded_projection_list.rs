//! One stored clause 4.21 projection string in the expanded stage.

use crate::projection::expanded_projection_term::ExpandedProjectionTerm;
use derive_more::{Deref, From, Into};
use serde::{Deserialize, Serialize};

/// The terms of one `pick` or `omit` string after expansion through the `@context` of the request that carried it.
///
/// Serialized as the array of its terms, the form a stored subscription keeps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Deref, From, Into)]
#[serde(transparent)]
pub struct ExpandedProjectionList(Vec<ExpandedProjectionTerm>);
