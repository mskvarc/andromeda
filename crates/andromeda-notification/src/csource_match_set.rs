//! The registrations one Context Source Registration Subscription currently
//! selects (ETSI GS CIM 009 clauses 5.11.3.4 and 5.11.7).
//!
//! Two senders need this set and need it to be the same set: clause 5.11.3.4
//! has an updated subscription notified with "all currently matching Context
//! Source Registrations", and clause 5.11.7 has a `timeInterval` subscription
//! notified with "all the Context Source Registrations whose information
//! property matches" on every elapsed interval. It is one question, asked from
//! two places, so it is answered once.

use crate::error::NotificationError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_registration::store::RegistrationStore;
use andromeda_subscription::{compiled::CompiledCsrSubscription, csource_matching::csource_subscription_selects_registration};

/// How many registrations one page of the sweep reads.
const MATCH_SET_PAGE_SIZE: u32 = 500;

/// Every registration `subscription` selects, in identifier order.
///
/// The registrations are swept by keyset over their identifier, so a
/// registration created or removed while the sweep runs can neither shift a
/// later page nor make one registration appear twice.
///
/// The lifecycle of the subscription is not consulted: whether an expired or
/// paused subscription is notified at all is the caller's question
/// ([`csource_subscription_is_live`](andromeda_subscription::csource_matching::csource_subscription_is_live)),
/// and answering it here would hide it from the sender that has to report the
/// outcome.
///
/// # Errors
///
/// Returns [`NotificationError::Registration`] when the registration store
/// cannot be listed.
pub(crate) async fn matching_registrations(
    csr_repo: &dyn RegistrationStore,
    subscription: &CompiledCsrSubscription,
) -> Result<Vec<ContextSourceRegistration>, NotificationError> {
    let mut matching = Vec::new();
    let mut after: Option<Identifier> = None;
    loop {
        let page = csr_repo.find_all_active(after.as_ref(), MATCH_SET_PAGE_SIZE).await?;
        let full = page.len() >= usize::try_from(MATCH_SET_PAGE_SIZE).unwrap_or(usize::MAX);
        after = page.last().map(|registration| registration.id.clone()); // clone: the next page starts after it, while the page itself moves below
        matching.extend(
            page.into_iter()
                .filter(|registration| csource_subscription_selects_registration(subscription, registration)),
        );
        if !full {
            break;
        }
    }
    Ok(matching)
}
