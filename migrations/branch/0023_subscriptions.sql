-- ============================================================================
-- 0023: Subscriptions and their delivery state
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.3.6.8, 4.8, 5.2.12, 5.2.14.2, 5.2.15,
--   5.8
--
-- Entity subscriptions and context source registration subscriptions, and the
-- delivery statistics of each.
--
-- Codes:
--
--   kind: 1 entity, 2 csource
--   status: 1 active, 2 paused, 3 expired
--   notification_trigger: 1 entityCreated, 2 entityUpdated, 3 entityDeleted,
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
-- A `subscriptions` row changes only when the client updates it and when it
-- expires. `modified_at` (clause 4.8) moves only on a client update, never on
-- a delivery.
-- ============================================================================

CREATE TABLE subscriptions (
    -- The identity every queued delivery references in place of the URI.
    subscription_key BIGINT GENERATED ALWAYS AS IDENTITY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- A csource subscription leaves `dataset_id` and `local_only` at their
    -- defaults.
    kind SMALLINT NOT NULL,
    status SMALLINT NOT NULL,
    local_only BOOLEAN NOT NULL DEFAULT FALSE,
    -- Table 5.2.12-1 `splitEntities`. NULL is the absent member, which defers
    -- to the deployment default, so the column has no default of its own.
    split_entities BOOLEAN,
    -- NULL means the subscription never expires.
    expires_at TIMESTAMPTZ,
    -- Seconds.
    time_interval DOUBLE PRECISION,
    -- Seconds.
    throttling DOUBLE PRECISION,
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    -- The endpoint's clause 5.2.15 `cooldown` and `timeout` stay inside this
    -- document: they are read once per delivery from the subscription already
    -- in memory, and never queried across rows.
    notification JSONB COMPRESSION lz4 NOT NULL,
    subscription_name TEXT COMPRESSION lz4,
    description TEXT COMPRESSION lz4,
    entities JSONB COMPRESSION lz4,
    -- `attribute_names` keys, in the order the subscriber wrote them. NULL
    -- means every attribute.
    watched_attributes INTEGER[] COMPRESSION lz4,
    -- What the subscriber wrote and nothing else. NULL is the absent member,
    -- which reads as the clause 5.2.12 default, `attributeCreated` and
    -- `attributeUpdated`, wherever a match is decided. An empty selection
    -- could never fire and is refused before it is written.
    notification_trigger SMALLINT[] COMPRESSION lz4,
    q TEXT COMPRESSION lz4,
    geo_q JSONB COMPRESSION lz4,
    scope_q TEXT COMPRESSION lz4,
    csf TEXT COMPRESSION lz4,
    -- `attribute_names` keys of clause 5.2.12 `expandValues` and `jsonKeys`,
    -- in the order the subscriber wrote them. They are kept for the
    -- representation only: the coercion they ask for is applied to `q` when
    -- the subscription is written, so `q` is already the expression matching
    -- evaluates.
    expand_values INTEGER[] COMPRESSION lz4,
    json_keys INTEGER[] COMPRESSION lz4,
    dataset_id TEXT[] COMPRESSION lz4,
    -- Used by csource subscriptions only.
    temporal_q JSONB COMPRESSION lz4,
    lang TEXT COMPRESSION lz4,
    -- The `@context` the subscription was created under, which notifications
    -- are compacted with.
    jsonld_context TEXT COMPRESSION lz4,
    -- Table 5.2.12-1 `ngsildConformance`, the `major.minor` text the client
    -- wrote. When set, a notification undergoes the clause 4.3.6.8 backwards
    -- compatibility operation before it is sent (clause 5.8.6). NULL means no
    -- amendment.
    ngsild_conformance TEXT COMPRESSION lz4,
    CONSTRAINT pk_subscriptions PRIMARY KEY (subscription_key),
    CONSTRAINT uq_subscriptions_id UNIQUE (id),
    CONSTRAINT chk_subscriptions_id_length CHECK (octet_length(id) <= 1024),
    CONSTRAINT chk_subscriptions_notification_trigger CHECK (
        notification_trigger IS NULL
        OR (cardinality(notification_trigger) > 0
            AND notification_trigger <@ ARRAY[1, 2, 3, 4, 5, 6]::SMALLINT[])),
    CONSTRAINT chk_subscriptions_kind CHECK (kind BETWEEN 1 AND 2),
    CONSTRAINT chk_subscriptions_status CHECK (status BETWEEN 1 AND 3),
    CONSTRAINT chk_subscriptions_time_interval_positive CHECK (time_interval > 0),
    CONSTRAINT chk_subscriptions_throttling_positive CHECK (throttling > 0),
    CONSTRAINT chk_subscriptions_time_interval_excludes_throttling
        CHECK (NOT (time_interval IS NOT NULL AND throttling IS NOT NULL)),
    CONSTRAINT chk_subscriptions_time_interval_excludes_watched_attributes
        CHECK (NOT (time_interval IS NOT NULL AND watched_attributes IS NOT NULL))
);

-- Loading the subscriptions of one kind and status for matching.
CREATE INDEX idx_subscriptions_kind_status ON subscriptions(kind, status);

CREATE INDEX idx_subscriptions_expires_at ON subscriptions(expires_at) WHERE expires_at IS NOT NULL;

-- The delivery statistics of clause 5.2.14.2, created in the
-- subscription's own create transaction. `times_sent` and `last_notification`
-- move on every claimed notification, `last_success` on a delivery the
-- endpoint accepted, and `times_failed` with `last_failure` on one it did
-- not. A claim also reads `last_failure`: it is refused while that instant
-- lies inside the endpoint's clause 5.2.15 `cooldown` window, which suppresses
-- the notification entirely.
CREATE TABLE subscription_delivery_state (
    subscription_key BIGINT NOT NULL,
    times_sent INTEGER NOT NULL DEFAULT 0,
    times_failed INTEGER NOT NULL DEFAULT 0,
    last_notification TIMESTAMPTZ,
    last_success TIMESTAMPTZ,
    last_failure TIMESTAMPTZ,
    CONSTRAINT pk_subscription_delivery_state PRIMARY KEY (subscription_key),
    CONSTRAINT fk_subscription_delivery_state_subscription
        FOREIGN KEY (subscription_key)
        REFERENCES subscriptions(subscription_key) ON DELETE CASCADE,
    -- The floor is zero, not the "greater than 0" of Table 5.2.14.2-1. That
    -- table restricts the `timesSent` and `timesFailed` members of a rendered
    -- NotificationParams, which are optional, so a counter that has not moved
    -- is an absent member; the column must hold a value before it first
    -- advances, so it holds zero and the representation omits it.
    CONSTRAINT chk_subscription_delivery_state_times_sent_non_negative CHECK (times_sent >= 0),
    CONSTRAINT chk_subscription_delivery_state_times_failed_non_negative CHECK (times_failed >= 0)
) WITH (
    fillfactor = 50,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 100,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 100,
    autovacuum_vacuum_cost_delay = 0
);
