//! Expansion of the clause 4.21 projection lists of a subscription's
//! notification (Table 5.2.14.1-1 `pick` and `omit`, ETSI GS CIM 009 clause
//! 5.5.7).

use crate::error::SubscriptionError;
use andromeda_ngsi_ld_query::{
    projection::{
        exclusivity::check_member_exclusivity,
        expanded_projection_list::ExpandedProjectionList,
        expanded_projection_term::ExpandedProjectionTerm,
        expansion::expand_projection_terms,
        written_projection_list::WrittenProjectionList,
    },
    query_error::QueryError,
    term_expander::TermExpander,
};

/// Expand the `pick` and `omit` lists a notification names, each absent where it
/// names none, refusing a member both name once expanded (clause 4.21).
///
/// Every attribute name expands, at every nesting level, and the clause 4.21
/// keywords stay as written. The term tree is what the subscription stores,
/// since an expanded name may hold the separators of the clause 4.21 text, and
/// every expanded name is held within the identifier bound a stored name keeps.
///
/// # Errors
///
/// Returns [`SubscriptionError::Query`] when a name does not expand, or is
/// past the bound, or a member is picked and omitted.
pub(crate) async fn expand_notification_projection(
    pick: Option<&[WrittenProjectionList]>,
    omit: Option<&[WrittenProjectionList]>,
    expander: &impl TermExpander,
) -> Result<(Option<Vec<ExpandedProjectionList>>, Option<Vec<ExpandedProjectionList>>), SubscriptionError> {
    let pick = expand_projection_lists(pick, expander).await?;
    let omit = expand_projection_lists(omit, expander).await?;
    check_member_exclusivity(&members(pick.as_deref()), &members(omit.as_deref())).map_err(QueryError::from)?;
    Ok((pick, omit))
}

/// The terms of every list, as the one slice the exclusivity check reads.
fn members(lists: Option<&[ExpandedProjectionList]>) -> Vec<ExpandedProjectionTerm> {
    // clone: the exclusivity check reads the terms of every list as one slice
    lists.into_iter().flatten().flat_map(|list| list.iter().cloned()).collect()
}

/// Expand each list, one stored list per projection string.
async fn expand_projection_lists(
    lists: Option<&[WrittenProjectionList]>,
    expander: &impl TermExpander,
) -> Result<Option<Vec<ExpandedProjectionList>>, SubscriptionError> {
    let Some(lists) = lists else {
        return Ok(None);
    };
    let mut kept = Vec::with_capacity(lists.len());
    for list in lists {
        let terms = expand_projection_terms(list, expander).await?;
        terms.iter().try_for_each(ExpandedProjectionTerm::check_name_bound).map_err(QueryError::from)?;
        kept.push(ExpandedProjectionList::from(terms));
    }
    Ok(Some(kept))
}

#[cfg(test)]
mod tests {
    use crate::{error::SubscriptionError, projection_expansion::expand_notification_projection};
    use andromeda_ngsi_ld_query::{
        namespace_expander::EXAMPLE_NAMESPACE,
        projection::{expanded_projection_list::ExpandedProjectionList, projection_error::ProjectionError, written_projection_list::WrittenProjectionList},
        query_error::QueryError,
    };
    use serde_json::json;

    fn lists(raw: &[&str]) -> Vec<WrittenProjectionList> {
        raw.iter().map(|list| list.parse().expect("valid projection list")).collect()
    }

    async fn expanded(pick: &[&str], omit: &[&str]) -> Result<(Vec<ExpandedProjectionList>, Vec<ExpandedProjectionList>), SubscriptionError> {
        let (pick, omit) = expand_notification_projection(Some(&lists(pick)), Some(&lists(omit)), &EXAMPLE_NAMESPACE).await?;
        Ok((pick.unwrap_or_default(), omit.unwrap_or_default()))
    }

    /// Two strings of one member are one list, and a member is one member
    /// whichever spelling each list gives it.
    #[tokio::test]
    async fn a_member_named_in_both_pick_and_omit_under_any_spelling_is_refused() {
        for (pick, omit) in [
            (&["id"][..], &["id"][..]),
            (&["name", "speed"][..], &["type,https://example.com/speed"][..]),
            (&["owner{name}"][..], &["owner{https://example.com/name}"][..]),
        ] {
            let result = expanded(pick, omit).await;
            assert!(
                matches!(
                    &result,
                    Err(SubscriptionError::Query(QueryError::Projection(ProjectionError::MemberPickedAndOmitted { .. })))
                ),
                "{pick:?} {omit:?}: {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn pick_and_omit_of_different_members_are_stored_expanded() {
        let (pick, omit) = expanded(&["id,speed"], &["https://schema.org/speed"]).await.expect("different members");
        assert_eq!(
            serde_json::to_value(pick).expect("the lists write"),
            json!([[{"name": "id", "linked": []}, {"name": "https://example.com/speed", "linked": []}]])
        );
        assert_eq!(
            serde_json::to_value(omit).expect("the lists write"),
            json!([[{"name": "https://schema.org/speed", "linked": []}]])
        );
    }
}
