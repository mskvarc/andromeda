-- ============================================================================
-- 0021: Subscriptions and their delivery state
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.3.6.8, 4.8, 5.2.12, 5.2.14.2, 5.2.15,
--   5.8, 5.11.2.4, 5.11.3.4
--
-- Entity subscriptions and context source registration subscriptions, and the
-- delivery statistics of each.
--
-- Codes:
--
--   kind: 1 entity, 2 csource
--   status: 1 active, 2 paused, 3 expired
--   notification_triggers: 1 entityCreated, 2 entityUpdated, 3 entityDeleted,
--     4 attributeCreated, 5 attributeUpdated, 6 attributeDeleted
--
-- Clause 5.2.12 requires at least one of `entities` and `watched_attributes`.
-- The broker enforces that rather than a check, because the `localOnly`
-- exception depends on more than these columns. It also refuses a
-- `time_interval` combined with `watched_attributes` or `throttling`; the
-- checks below are the backstop that keeps such a row out.
--
-- `status` moves between active and paused as the client sets `isActive`,
-- which is not stored because it would always equal `status = active`, and
-- becomes expired when `expires_at` passes.
--
-- A `subscriptions` row changes only when the client updates it, when it
-- expires, and when an evaluation claims the notification a csource
-- subscription's latest write owes. `modified_at` (clause 4.8) moves on a
-- client update and when the expiry sweep expires the subscription, never on
-- a delivery.
--
-- A csource subscription is notified of each registration change against the
-- subscription as that change found it (clauses 5.11.2.4, 5.11.3.4). Every
-- client write of one takes the next `context_source_change_clock` revision,
-- the same counter every registration write takes, so the revisions order the
-- two kinds of write as they committed. A registration change is not matched
-- against a subscription written after it: that write's own notification
-- reads the registrations after both committed and reports the change.
-- ============================================================================
create table subscriptions (
    -- The identity every queued delivery references in place of the URI.
    subscription_key bigint generated always as identity,
    -- The `context_source_change_clock` revision of the subscription's latest
    -- client write. NULL exactly for an entity subscription.
    context_source_revision bigint,
    -- The `context_source_revision` whose notification an evaluation has
    -- claimed; the latest write is owed one notification, and an evaluation
    -- that reaches an older write's change after it has nothing left to send.
    -- NULL until the first claim.
    evaluated_context_source_revision bigint,
    created_at timestamptz not null,
    modified_at timestamptz not null,
    -- The transaction timestamp at which the subscription was created active
    -- or last set active from paused or expired. Clause 5.8.6 notifies only an
    -- active subscription, so a change committed before this instant is never
    -- notified to it, however late it is evaluated. The change's commit
    -- instant is the store's transaction timestamp too, so the two are
    -- ordered by one clock.
    active_since timestamptz not null,
    -- The transaction timestamp at which the subscription was created or the
    -- value of a member deciding which changes it is notified of last changed.
    -- A change matched only after it committed is not judged by a selection
    -- written after the change.
    selected_since timestamptz not null,
    -- NULL means the subscription never expires.
    expires_at timestamptz,
    -- Seconds.
    time_interval double precision,
    -- Seconds.
    throttling double precision,
    -- A csource subscription leaves `dataset_ids` and `local_only` at their
    -- defaults.
    kind smallint not null,
    status smallint not null,
    local_only boolean not null default false,
    -- Table 5.2.12-1 `splitEntities`. NULL is the absent member, which defers
    -- to the deployment default, so the column has no default of its own.
    split_entities boolean,
    id text compression lz4 collate "C" not null,
    -- The endpoint's clause 5.2.15 `cooldown` and `timeout` stay inside this
    -- document: they are read once per delivery from the subscription already
    -- in memory, and never queried across rows.
    notification json compression lz4 not null,
    subscription_name text compression lz4 collate "C",
    description text compression lz4 collate "C",
    entities json compression lz4,
    -- `attribute_names` keys, in the order the subscriber wrote them. NULL
    -- means every attribute.
    watched_attributes integer[] compression lz4,
    -- What the subscriber wrote and nothing else. NULL is the absent member,
    -- which reads as the clause 5.2.12 default, `attributeCreated` and
    -- `attributeUpdated`, wherever a match is decided. An empty selection
    -- could never fire and is refused before it is written.
    notification_triggers smallint[] compression lz4,
    q text compression lz4 collate "C",
    -- `q` again with every trailing `[...]` member as the subscriber wrote it,
    -- every other name as `q` spells it. Clause 4.9 reads a LanguageProperty's
    -- target language and a raw JSON key in that spelling, which the expanded
    -- `q` no longer carries. NULL exactly when `q` is.
    q_written_members text compression lz4 collate "C",
    geo_q json compression lz4,
    scope_q text compression lz4 collate "C",
    csf text compression lz4 collate "C",
    -- `attribute_names` keys of clause 5.2.12 `expandValues` and `jsonKeys`,
    -- in the order the subscriber wrote them. They are kept for the
    -- representation only: the coercion they ask for is applied to `q` when
    -- the subscription is written, so `q` is already the expression matching
    -- evaluates.
    expand_values integer[] compression lz4,
    json_keys integer[] compression lz4,
    dataset_ids text[] compression lz4 collate "C",
    -- Used by csource subscriptions only.
    temporal_q json compression lz4,
    lang text compression lz4 collate "C",
    -- The `@context` the subscription was created under, which notifications
    -- are compacted with.
    jsonld_context text compression lz4 collate "C",
    -- Table 5.2.12-1 `ngsildConformance`, the `major.minor` text the client
    -- wrote. When set, a notification undergoes the clause 4.3.6.8 backwards
    -- compatibility operation before it is sent (clause 5.8.6). NULL means no
    -- amendment.
    ngsild_conformance text compression lz4 collate "C",
    constraint pk_subscriptions primary key (subscription_key),
    constraint uq_subscriptions_id unique (id),
    constraint chk_subscriptions_id_length check (octet_length(id) <= 1024),
    constraint chk_subscriptions_notification_triggers check (notification_triggers is null or (cardinality(notification_triggers) > 0 and notification_triggers <@ array[1, 2, 3, 4, 5, 6]::smallint[])),
    constraint chk_subscriptions_kind check (kind between 1 and 2),
    constraint chk_subscriptions_context_source_revision check ((kind = 2) = (context_source_revision is not null)),
    constraint chk_subscriptions_evaluated_context_source_revision check (evaluated_context_source_revision <= context_source_revision),
    constraint chk_subscriptions_status check (status between 1 and 3),
    constraint chk_subscriptions_time_interval_positive check (time_interval > 0),
    constraint chk_subscriptions_throttling_positive check (throttling > 0),
    constraint chk_subscriptions_time_interval_excludes_throttling check (not (time_interval is not null and throttling is not null)),
    constraint chk_subscriptions_time_interval_excludes_watched_attributes check (not (time_interval is not null and watched_attributes is not null)),
    constraint chk_subscriptions_q_written_members check ((q is null) = (q_written_members is null)),
    constraint chk_subscriptions_csource_defaults check (kind = 1 or (dataset_ids is null and not local_only)),
    constraint chk_subscriptions_temporal_q_csource check (temporal_q is null or kind = 2)
);

-- Loading the subscriptions of one kind and status for matching.
create index idx_subscriptions_kind_status on subscriptions (kind, status);

create index idx_subscriptions_expires_at on subscriptions (expires_at)
where
    expires_at is not null;

-- The delivery statistics of clause 5.2.14.2, created in the
-- subscription's own create transaction. `times_sent` and `last_notification`
-- move on every claimed notification, `last_success` on a delivery the
-- endpoint accepted, and `times_failed` with `last_failure` on one it did
-- not. A claim also reads `last_failure`: it is refused while that instant
-- lies inside the endpoint's clause 5.2.15 `cooldown` window, which suppresses
-- the notification entirely. `last_interval_tick` is not a clause 5.2.14.2
-- member: it is the sweep instant at which a `timeInterval` subscription last
-- answered its grid, by a notification or by finding nothing to notify. The
-- grid itself is anchored on `created_at`, so a sweep that runs late delays
-- one notification without moving the next (clause 5.8.6).
create table subscription_delivery_state (
    subscription_key bigint not null,
    times_sent bigint not null default 0,
    times_failed bigint not null default 0,
    last_notification timestamptz,
    last_success timestamptz,
    last_failure timestamptz,
    last_interval_tick timestamptz,
    constraint pk_subscription_delivery_state primary key (subscription_key),
    constraint fk_subscription_delivery_state_subscription foreign key (subscription_key) references subscriptions (subscription_key) on delete cascade,
    -- The floor is zero, not the "greater than 0" of Table 5.2.14.2-1. That
    -- table restricts the `timesSent` and `timesFailed` members of a rendered
    -- NotificationParams, which are optional, so a counter that has not moved
    -- is an absent member; the column must hold a value before it first
    -- advances, so it holds zero and the representation omits it.
    constraint chk_subscription_delivery_state_times_sent_non_negative check (times_sent >= 0),
    constraint chk_subscription_delivery_state_times_failed_non_negative check (times_failed >= 0)
)
with (
    vacuum_truncate = false,
    fillfactor = 50,
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 100,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 100,
    autovacuum_vacuum_cost_delay = 0
);
