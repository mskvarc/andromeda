-- ============================================================================
-- 006: Subscriptions
-- ============================================================================
-- NGSI-LD Subscription management (spec clause 5.2.12, operations 5.8.x).
--
-- A Subscription defines a standing query: when matching entities change,
-- the broker sends a notification to the configured endpoint.
--
-- Key NGSI-LD rules encoded here:
--   • At least one of `entities` or `watched_attributes` must be present
--     (enforced at the application layer, not via CHECK, because of the
--     localOnly exception).
--   • `time_interval` and `watched_attributes` are mutually exclusive
--     (application-layer validation).
--   • `time_interval` and `throttling` are mutually exclusive.
--   • `status` lifecycle: active → paused (via is_active=false) → expired
--     (when expires_at passes).
--
-- JSONB columns are used for nested spec structures that vary in shape:
--   • entities:     array of EntitySelector {type, id, idPattern}
--   • notification: NotificationParams {format, endpoint{uri, accept,
--                   receiverInfo, notifierInfo}, attributes, sysAttrs, ...}
--   • geo_q:        GeoQuery {georel, geometry, coordinates, geoproperty}
--   • temporal_q:   TemporalQuery (only for CSR subscriptions)
-- ============================================================================

CREATE TABLE subscriptions (
    -- Subscription URI, e.g. "urn:ngsi-ld:Subscription:mySubscription"
    id TEXT NOT NULL PRIMARY KEY,

    -- Always "Subscription" per spec
    type TEXT NOT NULL DEFAULT 'Subscription',

    -- Optional human-readable name
    subscription_name TEXT,

    -- Optional description
    description TEXT,

    -- EntitySelector[] — which entities to watch
    -- Example: [{"type": "Vehicle"}, {"type": "Parking", "idPattern": "urn:ngsi-ld:Parking:.*"}]
    entities JSONB,

    -- Attribute names (expanded IRIs) that trigger notifications when changed.
    -- NULL means "all attributes".
    watched_attributes TEXT[],

    -- What changes trigger a notification:
    -- "entityCreated", "entityUpdated", "entityDeleted",
    -- "attributeCreated", "attributeUpdated", "attributeDeleted"
    -- Default (application-layer): ["attributeCreated", "attributeUpdated"]
    notification_trigger TEXT[],

    -- Periodic notification interval (seconds). Mutually exclusive with
    -- watched_attributes and throttling.
    time_interval DOUBLE PRECISION,

    -- NGSI-LD query string (clause 4.9) that entities must satisfy
    q TEXT,

    -- Geo-spatial filter: {georel, geometry, coordinates, geoproperty}
    geo_q JSONB,

    -- Scope query string (clause 4.19)
    scope_q TEXT,

    -- Context source filter query string
    csf TEXT,

    -- When true, match only local entities (no distributed federation match).
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

    -- Subscription lifecycle state: "active", "paused", "expired"
    status TEXT NOT NULL DEFAULT 'active',

    -- Temporal query — only used for Context Source Registration Subscriptions
    temporal_q JSONB,

    -- Preferred language for notifications
    lang TEXT,

    -- The @context URI used when creating the subscription, needed for
    -- compacting notification payloads.
    jsonld_context TEXT,

    -- Notification delivery statistics (spec clause 5.2.15)
    times_sent INTEGER NOT NULL DEFAULT 0,
    last_notification TIMESTAMPTZ,
    last_success TIMESTAMPTZ,
    last_failure TIMESTAMPTZ,

    -- System-managed temporal properties
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Lookup active subscriptions for notification matching
CREATE INDEX idx_subscriptions_status ON subscriptions(status);

-- Efficiently find and expire subscriptions past their expiry
CREATE INDEX idx_subscriptions_expires_at ON subscriptions(expires_at);
