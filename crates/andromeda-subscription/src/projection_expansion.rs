//! Expansion of the clause 4.21 projection lists of a subscription's
//! notification (Table 5.2.14.1-1 `pick` and `omit`, ETSI GS CIM 009 clause
//! 5.5.7).

use crate::error::SubscriptionError;
use andromeda_ngsi_ld_query::{
    projection::{
        exclusivity::check_member_exclusivity,
        expansion::expand_projection_terms,
        grammar::{ProjectionList, RawProjectionTerm},
        term::ProjectionTerm,
    },
    term_expander::TermExpander,
};

/// Replace each projection list of a notification with the term tree it
/// expands to, refusing a `pick` and an `omit` that name one member once
/// expanded.
///
/// Every attribute name expands, at every nesting level, and the clause 4.21
/// keywords stay as written; the tree is what the subscription stores, since
/// an expanded name may hold the separators of the clause 4.21 text.
///
/// # Errors
///
/// Returns [`SubscriptionError::Query`] when a name does not expand to a usable
/// attribute name, or when `pick` and `omit` name one member.
pub(crate) async fn expand_notification_projection(
    pick: Option<&mut [ProjectionList]>,
    omit: Option<&mut [ProjectionList]>,
    expander: &impl TermExpander,
) -> Result<(), SubscriptionError> {
    let pick = expand_projection_lists(pick.unwrap_or_default(), expander).await?;
    let omit = expand_projection_lists(omit.unwrap_or_default(), expander).await?;
    Ok(check_member_exclusivity(&pick, &omit)?)
}

/// Replace each list with its expansion, answering every term the lists of
/// one member expand to.
async fn expand_projection_lists(lists: &mut [ProjectionList], expander: &impl TermExpander) -> Result<Vec<ProjectionTerm>, SubscriptionError> {
    let mut member_terms = Vec::new();
    for list in lists {
        let terms = expand_projection_terms(list, expander).await?;
        *list = terms.iter().map(RawProjectionTerm::try_from).collect::<Result<Vec<_>, _>>()?.into();
        member_terms.extend(terms);
    }
    Ok(member_terms)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{error::SubscriptionError, projection_expansion::expand_notification_projection};
    use andromeda_ngsi_ld_query::{error::QueryError, projection::grammar::ProjectionList, term_expander::TermExpander};
    use iri_rs::IriBuf;
    use std::future::{Future, ready};

    /// Expands a short name under `https://example.com/`, the way a request
    /// `@context` with that vocabulary does, and keeps a full IRI as written.
    struct ExampleVocabulary;

    impl TermExpander for ExampleVocabulary {
        fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            let expanded = if term.contains(':') {
                term.to_owned()
            } else {
                format!("https://example.com/{term}")
            };
            ready(Ok(IriBuf::new(expanded).expect("valid IRI")))
        }

        fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            self.expand_term(term)
        }
    }

    fn lists(raw: &[&str]) -> Vec<ProjectionList> {
        raw.iter().map(|list| list.parse().expect("valid projection list")).collect()
    }

    async fn expanded(pick: &[&str], omit: &[&str]) -> Result<(Vec<ProjectionList>, Vec<ProjectionList>), SubscriptionError> {
        let mut pick = lists(pick);
        let mut omit = lists(omit);
        expand_notification_projection(Some(&mut pick), Some(&mut omit), &ExampleVocabulary).await?;
        Ok((pick, omit))
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
                matches!(&result, Err(SubscriptionError::Query(QueryError::ProjectionMemberPickedAndOmitted { .. }))),
                "{pick:?} {omit:?}: {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn pick_and_omit_of_different_members_are_stored_expanded() {
        let (pick, omit) = expanded(&["id,speed"], &["https://schema.org/speed"]).await.expect("different members");
        assert_eq!(pick, lists(&["id,https://example.com/speed"]));
        assert_eq!(omit, lists(&["https://schema.org/speed"]));
    }
}
