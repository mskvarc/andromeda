-- ============================================================================
-- 0062: Retire `notification_outbox`, drain-first
-- ============================================================================
-- 0061 replaced the outbox with an event and its deliveries. This migration
-- removes what it replaced, and it does so without carrying anything across.
--
-- Why there is no backfill. An outbox row could only become a
-- `notification_events` row if the migration could reconstruct that event's
-- identity, which is `(write_id, event_ordinal)`. `notification_outbox` has no
-- write identity: 0037 named `write_id` on the temporal relations and left the
-- outbox untouched, so the atomic write that produced a queued notification is
-- not recorded anywhere on the row. Grouping the per-Subscription rows back
-- into one event per change is only lossless when that identity is present --
-- `(entity_id, trigger, entity_snapshot, previous_snapshot)` alone cannot
-- distinguish two genuinely separate writes that set an attribute back to the
-- value it already had, and collapsing those would silently drop a
-- notification a subscriber is owed. Minting a fresh identity per row is no
-- better: it defeats the replay dedupe the new unique constraint exists for.
--
-- The policy is therefore drain-first, the same one 0063 applies to the
-- side-effect queue: the outbox must be empty of undelivered work before this
-- migration runs. Deployment stops accepting writes, lets the delivery worker
-- finish the backlog, and then migrates. The first statement is that
-- requirement stated as an assertion -- it casts a diagnostic to `SMALLINT`,
-- which fails the migration with the offending row named and, because a
-- migration is one transaction, leaves the outbox exactly as it was. There is
-- no procedural logic here and none is wanted: the check is a query whose
-- result type cannot be produced.
--
-- Rows that were delivered and not yet reaped are discarded with the table.
-- They are the record of notifications that already arrived; the cleanup task
-- existed to remove them and would have, and they carry no identity from which
-- an event could be rebuilt in any case.
--
-- Exhausted rows -- undelivered, at or above the attempt ceiling -- are caught
-- by the assertion rather than exempted from it. An exhausted notification is
-- a notification a subscriber never received, and dropping that evidence in a
-- migration would hide a delivery failure behind a schema change. The operator
-- decides: retry them under the old build, or record and delete them by hand.
--
-- `DROP TABLE` takes the table's indexes with it, so `idx_outbox_subscription`
-- and `idx_outbox_delivered` (0013, 0018) and the claim-candidate index 0043
-- built need no statements of their own; 0044 already dropped
-- `idx_outbox_pending`. Nothing references `notification_outbox`: its only
-- foreign key pointed at `subscriptions`, in the direction that is removed
-- here.
-- ============================================================================

SELECT CAST('notification_outbox still holds undelivered notification '
            || o.id::text || ' for subscription ' || o.subscription_id
            || '; drain the outbox before migrating' AS SMALLINT)
FROM notification_outbox AS o
WHERE o.delivered_at IS NULL
LIMIT 1;

DROP TABLE notification_outbox;
