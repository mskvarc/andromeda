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
--   * `status` lifecycle: active -> paused -> expired (when expires_at passes).
--     The pause/resume input is the request `isActive`; it is not stored,
--     because a stored copy would always equal `status = 'active'`.
--
-- JSONB columns are used for nested spec structures that vary in shape:
--   * entities:     array of EntitySelector {type, id, idPattern}
--   * notification: NotificationParams {format, endpoint{uri, accept,
--                   cooldown, timeout, receiverInfo, notifierInfo},
--                   attributes, sysAttrs, ...}
--   * geo_q:        GeoQuery {georel, geometry, coordinates, geoproperty}
--   * temporal_q:   TemporalQuery (only for CSR subscriptions)
--
-- `subscription_key` is the narrow surrogate identity the notification queue
-- carries, and the primary key of this table. A delivery row exists once per
-- matched Subscription per affected Entity, so the queue is the
-- highest-cardinality relation that references a Subscription; repeating the
-- variable-length, locale-collated `id` URI in every one of those tuples is the
-- single largest avoidable cost on the delivery path. As the primary key it is
-- what a foreign key points at, so deleting a Subscription reclaims its queued
-- deliveries through ON DELETE CASCADE with no read-modify-write loop in the
-- broker.
--
-- The delivery counters of clauses 5.2.14.2 and 5.2.15 (`timesSent`,
-- `timesFailed`, `lastNotification`, `lastSuccess`, `lastFailure`) are not
-- columns of this table. Every notification sent moves them, and a popular
-- subscription is moved by many workers at once, so keeping them here would
-- rewrite the wide subscription row -- its JSONB selectors, its query strings,
-- its endpoint -- on every delivery. They live in
-- `subscription_delivery_state` below: one narrow row per subscription, keyed
-- by `subscription_key`, with no index but its primary key and a fillfactor of
-- 50, so every claim and every mark is a heap-only tuple update on a page that
-- keeps room for it. `subscriptions.modified_at` is the resource's clause 4.8
-- modification instant and moves only when the client changes the
-- subscription, never on delivery.
--
-- `id` is byte-collated: it is a URI, compared and matched byte-wise
-- everywhere the broker uses it, and byte order is what lets a prefix match
-- use its unique index. The variable-length JSONB and array columns are stored
-- with lz4 TOAST compression: they are written once per client update and read
-- on every cache reload, and lz4 decompresses far faster than pglz for the
-- same read.
--
-- Columns are laid out in descending alignment -- not-null fixed-width first,
-- then nullable fixed-width, then the varlena columns -- so the fixed-width
-- prefix of a row packs with no interior padding.
-- ============================================================================

CREATE TABLE subscriptions (
    -- Narrow surrogate identity referenced by the notification queue, and the
    -- primary key.
    subscription_key BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,

    -- System-managed temporal properties (clause 4.8).
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Discriminator: "entity" for entity subscriptions, "csource" for CSR
    -- subscriptions. Both types share the same schema; csource subscriptions
    -- leave dataset_id and local_only at their defaults (NULL / FALSE).
    kind subscription_kind NOT NULL,

    -- Subscription lifecycle state.
    status subscription_status NOT NULL DEFAULT 'active',

    -- When true, match only local entities (no distributed federation match).
    local_only BOOLEAN NOT NULL DEFAULT FALSE,

    -- Table 5.2.12-1 `splitEntities`: when true, single entities are assumed
    -- distributed across context sources, so the filters can only be applied
    -- after re-assembly (clause 5.8.6). Tri-state on the wire -- an absent value
    -- defers to the deployment default -- so the column is nullable, not defaulted.
    split_entities BOOLEAN,

    -- Subscription expiry. NULL means perpetual.
    -- When NOW() > expires_at, status transitions to "expired".
    expires_at TIMESTAMPTZ,

    -- Periodic notification interval (seconds).
    -- Mutually exclusive with watched_attributes and throttling.
    time_interval DOUBLE PRECISION,

    -- Minimum seconds between consecutive notifications.
    throttling DOUBLE PRECISION,

    -- Subscription URI, e.g. "urn:ngsi-ld:Subscription:mySubscription".
    id TEXT COLLATE "C" NOT NULL,

    -- Notification configuration (required by spec):
    -- {format, endpoint: {uri, accept, cooldown, timeout, receiverInfo,
    --  notifierInfo}, attributes, sysAttrs, showChanges, ...}
    -- The endpoint's clause 5.2.15 `cooldown` and `timeout` live here rather
    -- than in columns of their own: they are read once per delivery, on the
    -- subscription the cache already holds, and never queried across rows.
    notification JSONB COMPRESSION lz4 NOT NULL,

    -- Optional human-readable name.
    subscription_name TEXT,

    -- Optional description.
    description TEXT,

    -- EntitySelector[] -- which entities to watch.
    -- Example: [{"type": "Vehicle"}, {"type": "Parking", "idPattern": "..."}]
    entities JSONB COMPRESSION lz4,

    -- Attribute names (expanded IRIs) that trigger notifications when changed.
    -- NULL means "all attributes".
    watched_attributes TEXT[] COMPRESSION lz4,

    -- What changes trigger a notification, as the clause-5.2.12 registry codes
    -- 1..6: entityCreated=1, entityUpdated=2, entityDeleted=3,
    -- attributeCreated=4, attributeUpdated=5, attributeDeleted=6. Clause 5.2.12
    -- defaults an absent member to attributeCreated plus attributeUpdated.
    -- The codes are the same registry the notification queue's `trigger`
    -- column is numbered from. Cardinality 0..1 (Table 5.2.12-1): the column
    -- records what the Context Subscriber wrote and nothing else -- NULL is the
    -- absent member, which `effective_notification_triggers` reads as the clause
    -- 5.2.12 default (attributeCreated plus attributeUpdated) wherever a match is
    -- decided, rather than materialising a selection the writer never made.
    notification_trigger SMALLINT[] COMPRESSION lz4,

    -- NGSI-LD query string (clause 4.9) that entities must satisfy.
    q TEXT,

    -- Geo-spatial filter: {georel, geometry, coordinates, geoproperty}.
    geo_q JSONB COMPRESSION lz4,

    -- Scope query string (clause 4.19).
    scope_q TEXT,

    -- Context source filter query string.
    csf TEXT,

    -- Attribute names (expanded IRIs) whose `q` operands were coerced through
    -- the subscription @context (clause 5.2.12 `expandValues`), and whose `q`
    -- object keys were expanded while their values were left uninterpreted
    -- (clause 5.2.12 `jsonKeys`). Both are stored for the representation only:
    -- the coercion they ask for is applied to `q` when the subscription is
    -- written, so `q` above is already the expression matching evaluates.
    expand_values TEXT[] COMPRESSION lz4,
    json_keys TEXT[] COMPRESSION lz4,

    -- Dataset IDs to filter notifications by.
    dataset_id TEXT[] COMPRESSION lz4,

    -- Temporal query -- only used for CSR subscriptions.
    temporal_q JSONB COMPRESSION lz4,

    -- Preferred language for notifications.
    lang TEXT,

    -- The @context URI used when creating the subscription, needed for
    -- compacting notification payloads.
    jsonld_context TEXT,

    -- Table 5.2.12-1 `ngsildConformance`: a `major.minor` version string that,
    -- when set, has the notification undergo the clause 4.3.6.8 backwards
    -- compatibility operation before it is sent (clause 5.8.6). Stored as the
    -- text the client wrote -- the form a retrieve echoes -- never compared in
    -- SQL. Nullable: an absent member means no amendment, with no default version.
    ngsild_conformance TEXT,

    -- `id` is the external identity a client registers and matches under; the
    -- surrogate key is the primary key, so `id` carries its own unique index.
    CONSTRAINT uq_subscriptions_id UNIQUE (id),

    -- Every stored trigger is one of the six clause-5.2.12 registry codes;
    -- mirrors the notification queue's `trigger` bound (0018).
    CONSTRAINT chk_subscriptions_notification_trigger CHECK (notification_trigger <@ ARRAY[1, 2, 3, 4, 5, 6]::SMALLINT[])
) WITH (
    fillfactor = 50,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 100,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 100,
    autovacuum_vacuum_cost_delay = 0
);

-- Lookup active subscriptions for notification matching (partitioned by kind)
CREATE INDEX idx_subscriptions_kind_status ON subscriptions(kind, status);

-- Efficiently find and expire subscriptions past their expiry. Perpetual
-- subscriptions carry NULL and are never candidates, so they are kept out of
-- the index.
CREATE INDEX idx_subscriptions_expires_at ON subscriptions(expires_at) WHERE expires_at IS NOT NULL;

-- Notification delivery statistics (spec clauses 5.2.14.2 and 5.2.15), one row
-- per subscription, created together with the subscription in the same
-- transaction and reclaimed with it through the cascade. `times_sent` and
-- `last_notification` move on every claimed notification slot; `last_success`
-- on a delivery the endpoint accepted, and `times_failed` together with
-- `last_failure` on one it did not. `last_failure` is also read on the way in:
-- a claim is refused while it lies inside the endpoint's clause 5.2.15
-- `cooldown` window, which is what suppresses the notification entirely. No
-- column here is indexed, and the fillfactor leaves half of every page free, so
-- each of those writes is a heap-only tuple update. The autovacuum settings are the
-- ones the queue relations use: the row count is small and the update rate
-- is the delivery rate, so the default scale factors would let dead tuples
-- outnumber live rows many times over before a vacuum ran.
CREATE TABLE subscription_delivery_state (
    subscription_key BIGINT PRIMARY KEY,
    times_sent INTEGER NOT NULL DEFAULT 0,
    times_failed INTEGER NOT NULL DEFAULT 0,
    last_notification TIMESTAMPTZ,
    last_success TIMESTAMPTZ,
    last_failure TIMESTAMPTZ,
    CONSTRAINT fk_subscription_delivery_state_subscription
        FOREIGN KEY (subscription_key)
        REFERENCES subscriptions(subscription_key) ON DELETE CASCADE,
    -- The floor is zero, not one, and that is not the "Greater than 0" of
    -- Table 5.2.14.2-1: the table restricts the `timesSent` and `timesFailed`
    -- *members* of a rendered NotificationParams, which are 0..1, so a counter
    -- that has not moved is an absent member. The column has no absence to
    -- offer -- a counter has to be somewhere before it is first advanced -- so
    -- it holds zero and the representation omits it.
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

-- Subscription forwarding mappings (clause 5.8.1.4). A broker keeps two mappings
-- for each local subscription that takes part in distributed operations: the
-- companion context source registration subscription that mirrors it, and the
-- identifier of the reduced copy placed at each matching registration's context
-- source. A deletion tears both down (clause 5.8.5.4); an inbound notification
-- is routed by the copy's identifier (clause 5.8.6). These mappings are tenant
-- global, not branch state: identifiers are `COLLATE "C"`, compared for equality
-- only.
CREATE TABLE subscription_companions (
    -- The local entity subscription.
    local_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL PRIMARY KEY,

    -- The companion context source registration subscription this broker minted.
    companion_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL UNIQUE
);

CREATE TABLE forwarded_subscriptions (
    -- The local entity subscription the copy reduces.
    local_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    -- The registration whose context source holds the copy.
    registration_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    -- The identifier the copy carries at the context source.
    remote_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL UNIQUE,

    PRIMARY KEY (local_subscription_id, registration_id)
);
