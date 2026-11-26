-- ============================================================================
-- 0048: Delivery holds without a stored probe
-- ============================================================================
-- A held Subscription's probe is its queued delivery first in change order:
-- the commit instant, then the write position, then the key. The queue reads
-- it from the change order index whenever a statement needs it, so a hold
-- names its Subscription alone, and however the failures of one endpoint are
-- recorded, the probe is the earliest change still owed.
--
-- Every hold is brought to that rule before the probe reference is dropped.
-- A held Subscription's probe is taken back into the claim order if it is set
-- aside, with the attempt count and the due instant it has. Every other
-- queued delivery of a held Subscription that holds no claim is set aside. A
-- delivery under a claim is left as it is, since a set-aside delivery holds
-- none, and the Subscription's next failure sets it aside.
-- ============================================================================
update
    notification_deliveries as d
set
    held_at = null
where
    d.held_at is not null
    and exists (
        select
            1
        from
            notification_delivery_holds as h
        where
            h.subscription_key = d.subscription_key)
    and not exists (
        select
            1
        from
            notification_deliveries as e
        where
            e.subscription_key = d.subscription_key
            and e.delivered_at is null
            and e.exhausted_at is null
            and (e.committed_at,
                e.event_ordinal,
                e.delivery_key) < (d.committed_at,
                d.event_ordinal,
                d.delivery_key));

update
    notification_deliveries as d
set
    held_at = now()
where
    d.held_at is null
    and d.delivered_at is null
    and d.exhausted_at is null
    and d.claimed_by is null
    and exists (
        select
            1
        from
            notification_delivery_holds as h
        where
            h.subscription_key = d.subscription_key)
    and exists (
        select
            1
        from
            notification_deliveries as e
        where
            e.subscription_key = d.subscription_key
            and e.delivered_at is null
            and e.exhausted_at is null
            and (e.committed_at,
                e.event_ordinal,
                e.delivery_key) < (d.committed_at,
                d.event_ordinal,
                d.delivery_key));

alter table notification_delivery_holds
    drop constraint uq_notification_delivery_holds_delivery,
    drop constraint fk_notification_delivery_holds_delivery,
    drop column delivery_key;
