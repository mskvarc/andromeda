//! `RedirectLedger` implementation over `subscription_receiver`, the
//! `subscription_companion_redirects` and `forwarded_subscription_redirects`.

use crate::{
    store_fault::{persistence_error, undecodable},
    subscription_forwarding::{
        redirect_statements::{
            adopted_receiver_statement,
            companion_redirected_statement,
            copy_redirected_statement,
            overdue_companions_statement,
            overdue_copies_statement,
            owe_companions_statement,
            owe_settled_copies_statement,
            postpone_companion_statement,
            postpone_copy_statement,
            record_receiver_statement,
        },
        repo::SubscriptionForwardingRepository,
    },
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use andromeda_subscription_forwarding::{
    error::SubscriptionForwardingError,
    receiver::ReceiverEndpoint,
    redirect_ledger::{OwedRedirect, RedirectLedger, RedirectTarget},
};
use futures::future::BoxFuture;
use std::time::Duration;

/// Parse a stored identifier back into a URI.
fn uri(raw: &str) -> Result<Identifier, SubscriptionForwardingError> {
    raw.parse::<Identifier>()
        .map_err(|error| SubscriptionForwardingError::from(undecodable(&error)))
}

impl RedirectLedger for SubscriptionForwardingRepository {
    fn adopt_receiver<'life0, 'life1, 'async_trait>(
        &'life0 self,
        receiver: &'life1 ReceiverEndpoint,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            // One transaction, so a receiver is never recorded without every
            // companion and copy placed before it owing it.
            let mut transaction = self
                .pool()
                .begin()
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            let adopted: Option<(String,)> = adopted_receiver_statement()
                .to_query_as()
                .fetch_optional(&mut *transaction)
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            if adopted.as_ref().is_some_and(|(adopted,)| adopted.as_str() == receiver.url().as_str()) {
                return Ok(());
            }
            owe_companions_statement()?
                .to_query()
                .execute(&mut *transaction)
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            owe_settled_copies_statement()?
                .to_query()
                .execute(&mut *transaction)
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            record_receiver_statement(receiver)?
                .to_query()
                .execute(&mut *transaction)
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            transaction
                .commit()
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            Ok(())
        })
    }

    fn overdue_redirects<'life0, 'async_trait>(&'life0 self, limit: u32) -> BoxFuture<'async_trait, Result<Vec<OwedRedirect>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let companions: Vec<(String,)> = overdue_companions_statement(limit)
                .to_query_as()
                .fetch_all(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            let copies: Vec<(String, String, String)> = overdue_copies_statement(limit)
                .to_query_as()
                .fetch_all(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            let companions = companions.iter().map(|(local,)| {
                Ok(OwedRedirect {
                    local: uri(local)?,
                    target: RedirectTarget::Companion,
                })
            });
            let copies = copies.iter().map(|(local, registration, remote)| {
                Ok(OwedRedirect {
                    local: uri(local)?,
                    target: RedirectTarget::Copy(ForwardedCopy {
                        registration: uri(registration)?,
                        remote_subscription: uri(remote)?,
                    }),
                })
            });
            companions.chain(copies).collect()
        })
    }

    fn redirected<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
        target: &'life2 RedirectTarget,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let executed = match target {
                RedirectTarget::Companion => companion_redirected_statement(local).to_query().execute(self.pool()).await,
                RedirectTarget::Copy(copy) => copy_redirected_statement(local, &copy.registration).to_query().execute(self.pool()).await,
            };
            executed.map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            Ok(())
        })
    }

    fn postpone_redirect<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
        target: &'life2 RedirectTarget,
        after: Duration,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let executed = match target {
                RedirectTarget::Companion => postpone_companion_statement(local, after).to_query().execute(self.pool()).await,
                RedirectTarget::Copy(copy) => postpone_copy_statement(local, &copy.registration, after).to_query().execute(self.pool()).await,
            };
            executed.map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            Ok(())
        })
    }
}
