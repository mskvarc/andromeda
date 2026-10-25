//! `SubscriptionForwardingStore` implementation over the two mapping tables.

use crate::{
    store_fault::{persistence_error, undecodable},
    subscription_forwarding::repo::SubscriptionForwardingRepository,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::subscription_forwarding::{ForwardedSubscriptions, SubscriptionCompanions};
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_registration::removal_hook::ForwardedSubscriptionCopy;
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use andromeda_subscription_forwarding::{error::SubscriptionForwardingError, mapping::SubscriptionForwardingStore};
use futures::future::BoxFuture;
use sea_query::{Expr, ExprTrait, OnConflict, Query, SelectStatement};
use sqlx::{Row, postgres::PgRow};

/// Every copy held at `registration`'s context source, served by
/// `idx_forwarded_subscriptions_registration`.
fn forwarded_for_registration_statement(registration: &Identifier) -> SelectStatement {
    Query::select()
        .column(ForwardedSubscriptions::LocalSubscriptionId)
        .column(ForwardedSubscriptions::RemoteSubscriptionId)
        .from(ForwardedSubscriptions::Table)
        .and_where(Expr::col(ForwardedSubscriptions::RegistrationId).eq(registration.as_str()))
        .to_owned()
}

/// Parse an identifier column back into a URI.
fn uri_column(row: &PgRow, column: &str) -> Result<Identifier, SubscriptionForwardingError> {
    let raw: String = row.try_get(column).map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
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
            let mut insert = Query::insert();
            insert
                .into_table(SubscriptionCompanions::Table)
                .columns([SubscriptionCompanions::LocalSubscriptionId, SubscriptionCompanions::CompanionSubscriptionId])
                .values([Expr::val(local.as_str()), Expr::val(companion.as_str())])
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?
                .on_conflict(
                    OnConflict::column(SubscriptionCompanions::LocalSubscriptionId)
                        .update_column(SubscriptionCompanions::CompanionSubscriptionId)
                        .to_owned(),
                );
            insert
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
            let row = Query::select()
                .column(SubscriptionCompanions::CompanionSubscriptionId)
                .from(SubscriptionCompanions::Table)
                .and_where(Expr::col(SubscriptionCompanions::LocalSubscriptionId).eq(local.as_str()))
                .to_owned()
                .to_query()
                .fetch_optional(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            row.map(|row| uri_column(&row, "companion_subscription_id")).transpose()
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
            let row = Query::select()
                .column(SubscriptionCompanions::LocalSubscriptionId)
                .from(SubscriptionCompanions::Table)
                .and_where(Expr::col(SubscriptionCompanions::CompanionSubscriptionId).eq(companion.as_str()))
                .to_owned()
                .to_query()
                .fetch_optional(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            row.map(|row| uri_column(&row, "local_subscription_id")).transpose()
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
            let mut insert = Query::insert();
            insert
                .into_table(ForwardedSubscriptions::Table)
                .columns([
                    ForwardedSubscriptions::LocalSubscriptionId,
                    ForwardedSubscriptions::RegistrationId,
                    ForwardedSubscriptions::RemoteSubscriptionId,
                ])
                .values([
                    Expr::val(local.as_str()),
                    Expr::val(copy.registration.as_str()),
                    Expr::val(copy.remote_subscription.as_str()),
                ])
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?
                .on_conflict(
                    OnConflict::columns([ForwardedSubscriptions::LocalSubscriptionId, ForwardedSubscriptions::RegistrationId])
                        .update_column(ForwardedSubscriptions::RemoteSubscriptionId)
                        .to_owned(),
                );
            insert
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
            let rows = Query::select()
                .column(ForwardedSubscriptions::RegistrationId)
                .column(ForwardedSubscriptions::RemoteSubscriptionId)
                .from(ForwardedSubscriptions::Table)
                .and_where(Expr::col(ForwardedSubscriptions::LocalSubscriptionId).eq(local.as_str()))
                .to_owned()
                .to_query()
                .fetch_all(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            rows.iter()
                .map(|row| {
                    Ok(ForwardedCopy {
                        registration: uri_column(row, "registration_id")?,
                        remote_subscription: uri_column(row, "remote_subscription_id")?,
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
            let rows = forwarded_for_registration_statement(registration)
                .to_query()
                .fetch_all(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            rows.iter()
                .map(|row| {
                    Ok(ForwardedSubscriptionCopy {
                        local_subscription: uri_column(row, "local_subscription_id")?,
                        remote_subscription: uri_column(row, "remote_subscription_id")?,
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
            let row = Query::select()
                .column(ForwardedSubscriptions::LocalSubscriptionId)
                .column(ForwardedSubscriptions::RegistrationId)
                .from(ForwardedSubscriptions::Table)
                .and_where(Expr::col(ForwardedSubscriptions::RemoteSubscriptionId).eq(remote_subscription.as_str()))
                .to_owned()
                .to_query()
                .fetch_optional(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            row.map(|row| Ok((uri_column(&row, "local_subscription_id")?, uri_column(&row, "registration_id")?)))
                .transpose()
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
            Query::delete()
                .from_table(ForwardedSubscriptions::Table)
                .and_where(Expr::col(ForwardedSubscriptions::LocalSubscriptionId).eq(local.as_str()))
                .and_where(Expr::col(ForwardedSubscriptions::RegistrationId).eq(registration.as_str()))
                .to_owned()
                .to_query()
                .execute(self.pool())
                .await
                .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
            Ok(())
        })
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::subscription_forwarding::store::forwarded_for_registration_statement;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_copies_held_at_a_registration_are_read_by_its_identifier() {
        let registration: Identifier = "urn:ngsi-ld:ContextSourceRegistration:b".parse().expect("a URI");
        let sql = forwarded_for_registration_statement(&registration).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "local_subscription_id", "remote_subscription_id" FROM "forwarded_subscriptions" WHERE "registration_id" = 'urn:ngsi-ld:ContextSourceRegistration:b'"#
        );
    }
}
