-- ============================================================================
-- 0014: Subscriptions
-- ============================================================================
-- NGSI-LD Subscription management (spec clause 5.2.12, operations 5.8.x).
--
-- A Subscription defines a standing query: when matching entities change,
-- the broker sends a notification to the configured endpoint.
--
-- Key NGSI-LD rules encoded here:
--   * At least one of `entities` or `watched_attributes` must be present
--     (enforced at the application layer, not via CHECK, because of the
--     localOnly exception)
--   * `time_interval` and `watched_attributes` are mutually exclusive
--     (application-layer validation)
--   * `time_interval` and `throttling` are mutually exclusive
--   * `status` lifecycle: active -> paused (via is_active=false) -> expired
--     (when expires_at passes)
--
-- JSONB columns are used for nested spec structures that vary in shape:
--   * entities:     array of EntitySelector {type, id, idPattern}
--   * notification: NotificationParams {format, endpoint{uri, accept,
--                   receiverInfo, notifierInfo}, attributes, sysAttrs, ...}
--   * geo_q:        GeoQuery {georel, geometry, coordinates, geoproperty}
--   * temporal_q:   TemporalQuery (only for CSR subscriptions)
--
-- `subscription_key` is the narrow surrogate identity the notification queue
-- carries. A delivery row exists once per matched Subscription per affected
-- Entity, so the queue is the highest-cardinality relation that references a
-- Subscription; repeating the variable-length, locale-collated `id` URI in
-- every one of those tuples is the single largest avoidable cost on the
-- delivery path. The UNIQUE constraint on it is what a foreign key needs to
-- point at, so deleting a Subscription reclaims its queued deliveries through
-- ON DELETE CASCADE with no read-modify-write loop in the broker.
-- ============================================================================

CREATE TABLE subscriptions (
    -- Subscription URI, e.g. "urn:ngsi-ld:Subscription:mySubscription"
    id TEXT NOT NULL PRIMARY KEY,

    -- Narrow surrogate identity referenced by the notification queue.
    subscription_key BIGINT GENERATED ALWAYS AS IDENTITY,

    -- Discriminator: "entity" for entity subscriptions, "csource" for CSR
    -- subscriptions. Both types share the same schema; csource subscriptions
    -- leave dataset_id and local_only at their defaults (NULL / FALSE).
    kind subscription_kind NOT NULL,

    -- Always "Subscription" per spec
    type TEXT NOT NULL DEFAULT 'Subscription',

    -- Optional human-readable name
    subscription_name TEXT,

    -- Optional description
    description TEXT,

    -- EntitySelector[] -- which entities to watch.
    -- Example: [{"type": "Vehicle"}, {"type": "Parking", "idPattern": "..."}]
    entities JSONB,

    -- Attribute names (expanded IRIs) that trigger notifications when changed.
    -- NULL means "all attributes".
    watched_attributes TEXT[],

    -- What changes trigger a notification:
    -- "entityCreated", "entityUpdated", "entityDeleted",
    -- "attributeCreated", "attributeUpdated", "attributeDeleted".
    -- Default (application-layer): ["attributeCreated", "attributeUpdated"]
    notification_trigger TEXT[],

    -- Periodic notification interval (seconds).
    -- Mutually exclusive with watched_attributes and throttling.
    time_interval DOUBLE PRECISION,

    -- NGSI-LD query string (clause 4.9) that entities must satisfy
    q TEXT,

    -- Geo-spatial filter: {georel, geometry, coordinates, geoproperty}
    geo_q JSONB,

    -- Scope query string (clause 4.19)
    scope_q TEXT,

    -- Context source filter query string
    csf TEXT,

    -- When true, match only local entities (no distributed federation match)
    local_only BOOLEAN NOT NULL DEFAULT FALSE,

    -- Dataset IDs to filter notifications by
    dataset_id TEXT[],

    -- Whether the subscription is currently active (maps to status)
    is_active BOOLEAN NOT NULL DEFAULT TRUE,

    -- Notification configuration (required by spec):
    -- {format, endpoint: {uri, accept, receiverInfo, notifierInfo},
    --  attributes, sysAttrs, showChanges, ...}
    notification JSONB NOT NULL,

    -- Subscription expiry. NULL means perpetual.
    -- When NOW() > expires_at, status transitions to "expired".
    expires_at TIMESTAMPTZ,

    -- Minimum seconds between consecutive notifications
    throttling DOUBLE PRECISION,

    -- Subscription lifecycle state
    status subscription_status NOT NULL DEFAULT 'active',

    -- Temporal query -- only used for CSR subscriptions
    temporal_q JSONB,

    -- Preferred language for notifications
    lang TEXT,

    -- The @context URI used when creating the subscription, needed for
    -- compacting notification payloads
    jsonld_context TEXT,

    -- Notification delivery statistics (spec clause 5.2.15)
    times_sent INTEGER NOT NULL DEFAULT 0,
    last_notification TIMESTAMPTZ,
    last_success TIMESTAMPTZ,
    last_failure TIMESTAMPTZ,

    -- System-managed temporal properties
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT uq_subscriptions_subscription_key UNIQUE (subscription_key)
);

-- Lookup active subscriptions for notification matching (partitioned by kind)
CREATE INDEX idx_subscriptions_kind_status ON subscriptions(kind, status);

-- Efficiently find and expire subscriptions past their expiry
CREATE INDEX idx_subscriptions_expires_at ON subscriptions(expires_at);
