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
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_query::pagination::{CountPolicy, Pagination};
use andromeda_registration::store::RegistrationStore;
use andromeda_subscription::{compiled::CompiledCsrSubscription, csource_matching::csource_subscription_selects_registration};

/// How many registrations one page of the sweep reads.
const MATCH_SET_PAGE_SIZE: u32 = 500;

/// Every registration `subscription` selects, in store order.
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
    let mut offset = 0;
    loop {
        let page = csr_repo
            .list(Pagination {
                limit: MATCH_SET_PAGE_SIZE,
                offset,
                count: CountPolicy::Emit,
            })
            .await?;
        if page.registrations.is_empty() {
            break;
        }
        matching.extend(
            page.registrations
                .into_iter()
                .filter(|registration| csource_subscription_selects_registration(subscription, registration)),
        );
        offset += MATCH_SET_PAGE_SIZE;
        if u64::from(offset) >= page.total {
            break;
        }
    }
    Ok(matching)
}
