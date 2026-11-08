//! `SubscriptionForwardingStore` implementation over the two mapping tables.

use crate::{
    store_fault::{persistence_error, undecodable},
    subscription_forwarding::{
        repo::SubscriptionForwardingRepository,
        statements::{
            companion_of_statement,
            forwarded_for_registration_statement,
            forwarded_of_statement,
            link_companion_statement,
            local_of_companion_statement,
            local_of_forwarded_statement,
            record_forwarded_statement,
            remove_forwarded_statement,
        },
    },
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_registration::removal_hook::ForwardedSubscriptionCopy;
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use andromeda_subscription_forwarding::{error::SubscriptionForwardingError, mapping::SubscriptionForwardingStore};
use futures::future::BoxFuture;

/// Parse a stored identifier back into a URI.
fn uri(raw: &str) -> Result<Identifier, SubscriptionForwardingError> {
    raw.parse::<Identifier>()
        .map_err(|error| SubscriptionForwardingError::from(undecodable(&error)))
}

impl SubscriptionForwardingStore for SubscriptionForwardingRepository {
    fn link_companion<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
        companion: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            link_companion_statement(local, companion)?
                .to_query()
                .execute(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            Ok(())
        })
    }

    fn companion_of<'life0, 'life1, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<Identifier>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let row: Option<(String,)> = companion_of_statement(local)
                .to_query_as()
                .fetch_optional(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            row.map(|(companion,)| uri(&companion)).transpose()
        })
    }

    fn local_of_companion<'life0, 'life1, 'async_trait>(
        &'life0 self,
        companion: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<Identifier>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let row: Option<(String,)> = local_of_companion_statement(companion)
                .to_query_as()
                .fetch_optional(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            row.map(|(local,)| uri(&local)).transpose()
        })
    }

    fn record_forwarded<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
        copy: &'life2 ForwardedCopy,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            record_forwarded_statement(local, copy)?
                .to_query()
                .execute(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            Ok(())
        })
    }

    fn forwarded_of<'life0, 'life1, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Vec<ForwardedCopy>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let rows: Vec<(String, String)> = forwarded_of_statement(local)
                .to_query_as()
                .fetch_all(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            rows.iter()
                .map(|(registration, remote_subscription)| {
                    Ok(ForwardedCopy {
                        registration: uri(registration)?,
                        remote_subscription: uri(remote_subscription)?,
                    })
                })
                .collect()
        })
    }

    fn forwarded_for_registration<'life0, 'life1, 'async_trait>(
        &'life0 self,
        registration: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Vec<ForwardedSubscriptionCopy>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let rows: Vec<(String, String)> = forwarded_for_registration_statement(registration)
                .to_query_as()
                .fetch_all(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            rows.iter()
                .map(|(local_subscription, remote_subscription)| {
                    Ok(ForwardedSubscriptionCopy {
                        local_subscription: uri(local_subscription)?,
                        remote_subscription: uri(remote_subscription)?,
                    })
                })
                .collect()
        })
    }

    fn local_of_forwarded<'life0, 'life1, 'async_trait>(
        &'life0 self,
        remote_subscription: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<(Identifier, Identifier)>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let row: Option<(String, String)> = local_of_forwarded_statement(remote_subscription)
                .to_query_as()
                .fetch_optional(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            row.map(|(local, registration)| Ok((uri(&local)?, uri(&registration)?))).transpose()
        })
    }

    fn remove_forwarded<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
        registration: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            remove_forwarded_statement(local, registration)
                .to_query()
                .execute(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            Ok(())
        })
    }
}
