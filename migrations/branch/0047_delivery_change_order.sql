-- ============================================================================
-- 0047: Delivery change order
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.2.12 and 5.8.6
--
-- A Subscription's deliveries are ordered by the changes they notify: the
-- commit instant each event records, then the event's position within its
-- write, with the delivery key deciding only between deliveries alike in both.
-- The key alone does not order them. Each connection draws keys from a block
-- of its own, so a later change can take a smaller key, and a replay queues
-- the change of an earlier commit after later ones. The claim reads no event,
-- so a delivery holds the instant and the position itself.
--
-- A delivery always names both. The columns are added with a constant
-- default, which fills them without rewriting the table, every delivery
-- already queued takes them from its event, and the defaults are dropped, so
-- a delivery queued later is refused unless it names them.
--
-- The two columns follow `last_error`, the variable-length column the table
-- ends with. Every row is NULL in `delivered_at` or in `exhausted_at`, ahead
-- of it, and PostgreSQL reaches every column past a row's first NULL by
-- walking the row whatever its place.
--
-- The index holds the queued rows of each Subscription in that order. It
-- serves the claim's test for a newer queued delivery of the Subscription and
-- the choice of the held delivery that probes next.
-- ============================================================================
alter table notification_deliveries
    add column committed_at timestamptz not null default '-infinity',
    add column event_ordinal integer not null default 0;

update
    notification_deliveries
set
    committed_at = notification_events.committed_at,
    event_ordinal = notification_events.event_ordinal
from
    notification_events
where
    notification_events.event_key = notification_deliveries.event_key;

alter table notification_deliveries
    alter column committed_at drop default,
    alter column event_ordinal drop default;

create index idx_notification_deliveries_change_order on notification_deliveries (subscription_key, committed_at, event_ordinal, delivery_key)
where
    delivered_at is null and exhausted_at is null;
