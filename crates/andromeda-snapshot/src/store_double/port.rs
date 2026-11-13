//! The store double's answers to the snapshot store port.

use crate::{
    error::SnapshotError,
    snapshot_invariant::SnapshotInvariant,
    store::{InsertSnapshotParams, SnapshotPreparationJob, SnapshotStore},
    store_double::{ClaimHold, QuotaCheck, Renewal, StoreDouble, entry::Stage},
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::snapshot_target::SnapshotTarget;
use andromeda_ngsi_ld_lexicon::{duration::Iso8601Duration, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::PurgeCandidate,
};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use std::{sync::atomic::Ordering, time::Duration};
use url::Url;

/// The answer of an operation no test exercises.
fn unused<T: Send + 'static>(method: &'static str) -> BoxFuture<'static, Result<T, SnapshotError>> {
    Box::pin(async move { panic!("StoreDouble::{method} is not exercised by the policy tests") })
}

impl SnapshotStore for StoreDouble {
    fn get<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Identifier) -> BoxFuture<'async_trait, Result<Option<Snapshot>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        let found = {
            let entries = self.entries();
            {
                entries.iter().find(|entry| entry.snapshot.id == *id).map(|entry| Snapshot {
                    status: entry.status(),
                    ..entry.snapshot.clone()
                })
            }
        };
        Box::pin(async move { Ok(found) })
    }

    fn touch_last_used<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Identifier) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async { Ok(()) })
    }

    fn restart_preparation<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _id: &'life1 Identifier,
        _claimed_by: LeaseHolder,
        _lease: Duration,
    ) -> BoxFuture<'async_trait, Result<bool, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        self.restarts.fetch_add(1, Ordering::SeqCst);
        let held = self.claim == ClaimHold::Held;
        Box::pin(async move { Ok(held) })
    }

    fn release_preparation<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
        _claimed_by: LeaseHolder,
    ) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        for entry in self
            .entries()
            .iter_mut()
            .filter(|entry| entry.snapshot.id == *id && entry.stage == Stage::Claimed)
        {
            entry.stage = Stage::Waiting;
        }
        Box::pin(async move { Ok(()) })
    }

    fn update_status<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
        _claimed_by: LeaseHolder,
        status: SnapshotStatus,
        queries_details: Option<&'life2 [ExecutionResultDetails]>,
        temporal_queries_details: Option<&'life3 [ExecutionResultDetails]>,
    ) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
    {
        let outcome = self.end_preparation(id, status, queries_details, temporal_queries_details);
        Box::pin(async move { Ok(outcome) })
    }

    fn delete<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Identifier) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        let deleted = {
            let mut entries = self.entries();
            {
                let before = entries.len();
                entries.retain(|entry| entry.snapshot.id != *id);
                u64::try_from(before - entries.len()).unwrap_or(u64::MAX)
            }
        };
        Box::pin(async move { Ok(deleted) })
    }

    fn create<'life0, 'life1, 'async_trait>(&'life0 self, params: InsertSnapshotParams<'life1>) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        self.insert(&params);
        Box::pin(async { Ok(()) })
    }

    fn exists<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Identifier) -> BoxFuture<'async_trait, Result<bool, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        let held = self.entries().iter().any(|entry| entry.snapshot.id == *id);
        Box::pin(async move { Ok(held) })
    }

    fn target<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Identifier) -> BoxFuture<'async_trait, Result<Option<SnapshotTarget>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        unused("target")
    }

    fn published_targets<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<Vec<SnapshotTarget>, SnapshotError>>
    where
        'life0: 'async_trait,
    {
        unused("published_targets")
    }

    fn staging_target<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _id: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<SnapshotTarget>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        unused("staging_target")
    }

    fn next_preparing<'life0, 'async_trait>(
        &'life0 self,
        _claimed_by: LeaseHolder,
        _lease: Duration,
    ) -> BoxFuture<'async_trait, Result<Option<SnapshotPreparationJob>, SnapshotError>>
    where
        'life0: 'async_trait,
    {
        let job = self.claim_next();
        Box::pin(async move { Ok(job) })
    }

    fn claim_created<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
        _claimed_by: LeaseHolder,
        _lease: Duration,
    ) -> BoxFuture<'async_trait, Result<Option<SnapshotPreparationJob>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        let job = self.claim_created(id);
        Box::pin(async move { Ok(job) })
    }

    fn renew_preparation_lease<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _id: &'life1 Identifier,
        _claimed_by: LeaseHolder,
        _lease: Duration,
    ) -> BoxFuture<'async_trait, Result<bool, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        let Some(renewal) = self.renewal.clone() else {
            return unused("renew_preparation_lease");
        };
        self.renewals.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            match renewal {
                Renewal::Retained => Ok(true),
                Renewal::GatedOn(gate) => {
                    gate.notified().await;
                    Ok(true)
                }
                Renewal::Lost => Ok(false),
                Renewal::Fails => Err(SnapshotError::StoreSaturated),
            }
        })
    }

    fn update_metadata<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        _id: &'life1 Identifier,
        _lifetime: Option<&'life2 Iso8601Duration>,
        _priority: Option<SnapshotPriority>,
        _endpoint: Option<Option<&'life3 Url>>,
        _receiver_info: Option<Option<Vec<KeyValuePair>>>,
        _new_expires_at: Option<DateTime<Utc>>,
    ) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
    {
        unused("update_metadata")
    }

    fn promote_capture<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
        _claimed_by: LeaseHolder,
        status: SnapshotStatus,
        queries_details: Option<&'life2 [ExecutionResultDetails]>,
        temporal_queries_details: Option<&'life3 [ExecutionResultDetails]>,
    ) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
    {
        let promoted = match self.end_preparation(id, status, queries_details, temporal_queries_details) {
            0 => Err(SnapshotInvariant::ClaimLost { snapshot: id.clone() }.into()),
            _ended => Ok(()),
        };
        Box::pin(async move { promoted })
    }

    fn purge_candidates<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<Vec<PurgeCandidate>, SnapshotError>>
    where
        'life0: 'async_trait,
    {
        unused("purge_candidates")
    }

    fn delete_one<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Identifier) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        unused("delete_one")
    }

    fn promote_clone<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        source_id: &'life1 Identifier,
        target_id: &'life2 Identifier,
        _claimed_by: LeaseHolder,
    ) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        let copied = self.copy_clone(source_id, target_id);
        Box::pin(async move { copied })
    }

    fn delete_expired<'life0, 'async_trait>(&'life0 self, _cutoff: DateTime<Utc>) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
    {
        unused("delete_expired")
    }

    fn enforce_quotas<'life0, 'async_trait>(
        &'life0 self,
        _count_quota: Option<usize>,
        _byte_quota: Option<u64>,
    ) -> BoxFuture<'async_trait, Result<Vec<Identifier>, SnapshotError>>
    where
        'life0: 'async_trait,
    {
        let answer = match self.quota {
            QuotaCheck::Within => Ok(Vec::new()),
            QuotaCheck::Fails => Err(SnapshotError::StoreSaturated),
        };
        Box::pin(async { answer })
    }
}
