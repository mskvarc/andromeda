-- ============================================================================
-- 0024: Notification queue
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.2.12, 5.2.14.1, 5.6.6
--
-- The durable notification queue, as two relations: the payload belongs to
-- the change, and only the queue state belongs to the delivery. An Entity
-- watched by twenty Subscriptions produces one event and twenty deliveries.
-- The Entity's state and, under `showChanges` (clause 5.2.14.1), its previous
-- state are written once per change, while every claim, retry and
-- acknowledgement rewrites a narrow delivery row.
--
-- Codes:
--
--   trigger: 1 entityCreated, 2 entityUpdated, 3 entityDeleted,
--     4 attributeCreated, 5 attributeUpdated, 6 attributeDeleted
--
-- An event that no Subscription matches is never written, so no stored event
-- is without a delivery until its deliveries have been reaped.
-- ============================================================================

-- One row per change, written in the Entity's own transaction and never
-- updated. Claims never touch it, so its payloads are read from a heap no
-- queue traffic churns. It is reached only by primary key from a claimed
-- delivery, or by write identity when a replay checks whether its events are
-- already there.
CREATE TABLE notification_events (
    event_key BIGINT GENERATED ALWAYS AS IDENTITY,
    created_at TIMESTAMPTZ NOT NULL,
    -- The event's deterministic position within its write. With `write_id`
    -- it makes a replayed write insert nothing rather than announce a change
    -- a second time.
    event_ordinal INTEGER NOT NULL,
    trigger SMALLINT NOT NULL,
    write_id UUID NOT NULL,
    -- No foreign key: clause 5.6.6 deletion notifies about an Entity that no
    -- longer exists, and a reference would either forbid that row or cascade
    -- away the announcement of the deletion.
    entity_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    -- `attribute_names` keys. Empty, never NULL, for an Entity-level trigger:
    -- "no attribute in particular" is true there, and "unknown" is not.
    changed_attribute_name_keys INTEGER[] COMPRESSION lz4 NOT NULL,
    changed_dataset_id TEXT COMPRESSION lz4 COLLATE "C",
    entity_snapshot JSONB COMPRESSION lz4,
    previous_snapshot JSONB COMPRESSION lz4,
    CONSTRAINT pk_notification_events PRIMARY KEY (event_key),
    CONSTRAINT uq_notification_events_write_identity UNIQUE (write_id, event_ordinal),
    CONSTRAINT chk_notification_events_event_ordinal CHECK (event_ordinal >= 0),
    CONSTRAINT chk_notification_events_trigger CHECK (trigger BETWEEN 1 AND 6),
    CONSTRAINT chk_notification_events_entity_id_length CHECK (octet_length(entity_id) <= 1024),
    CONSTRAINT chk_notification_events_changed_dataset_id_length CHECK (octet_length(changed_dataset_id) <= 1024)
) WITH (
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The queue state, one row per matched Subscription, and the only relation a
-- claim scans, locks or updates.
CREATE TABLE notification_deliveries (
    delivery_key BIGINT GENERATED ALWAYS AS IDENTITY,
    event_key BIGINT NOT NULL,
    subscription_key BIGINT NOT NULL,
    next_attempt_at TIMESTAMPTZ NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    claimed_at TIMESTAMPTZ,
    lease_until TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,
    exhausted_at TIMESTAMPTZ,
    claimed_by UUID,
    last_error TEXT COMPRESSION lz4,
    CONSTRAINT pk_notification_deliveries PRIMARY KEY (delivery_key),
    CONSTRAINT fk_notification_deliveries_event
        FOREIGN KEY (event_key)
        REFERENCES notification_events(event_key) ON DELETE CASCADE,
    CONSTRAINT fk_notification_deliveries_subscription
        FOREIGN KEY (subscription_key)
        REFERENCES subscriptions(subscription_key) ON DELETE CASCADE,
    CONSTRAINT chk_notification_deliveries_attempt_count_non_negative
        CHECK (attempt_count >= 0),
    CONSTRAINT chk_notification_deliveries_claim_coherent
        CHECK (num_nonnulls(claimed_at, claimed_by, lease_until) IN (0, 3)),
    CONSTRAINT chk_notification_deliveries_exhausted_state CHECK (
        exhausted_at IS NULL
        OR (delivered_at IS NULL AND num_nonnulls(claimed_at, claimed_by, lease_until) = 0)),
    CONSTRAINT chk_notification_deliveries_last_error_length
        CHECK (last_error IS NULL OR char_length(last_error) <= 2048)
) WITH (
    fillfactor = 80,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The claim: rows still in the queue, due and unheld, in the order they became
-- claimable. The expression is the later of when a row is due and when its
-- lease lapses, so a claim is one range scan as deep as its batch.
CREATE INDEX idx_notification_deliveries_claim
    ON notification_deliveries (GREATEST(next_attempt_at, COALESCE(lease_until, '-infinity'::timestamptz)), delivery_key)
    WHERE delivered_at IS NULL AND exhausted_at IS NULL;

-- The reap of delivered rows.
CREATE INDEX idx_notification_deliveries_delivered_at
    ON notification_deliveries (delivered_at)
    WHERE delivered_at IS NOT NULL;

-- The reap of dead-lettered rows.
CREATE INDEX idx_notification_deliveries_exhausted_at
    ON notification_deliveries (exhausted_at)
    WHERE exhausted_at IS NOT NULL;

CREATE INDEX idx_notification_deliveries_subscription
    ON notification_deliveries (subscription_key);

-- The event cascade, and the check the reap runs for events whose last
-- delivery it removed.
CREATE INDEX idx_notification_deliveries_event
    ON notification_deliveries (event_key);
