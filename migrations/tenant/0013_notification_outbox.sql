CREATE TABLE notification_outbox (
    id BIGSERIAL PRIMARY KEY,
    subscription_id TEXT NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
    entity_id TEXT NOT NULL,
    trigger TEXT NOT NULL,
    entity_snapshot JSONB,
    previous_snapshot JSONB,
    changed_attrs TEXT[],
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    claimed_at TIMESTAMPTZ,
    claimed_by TEXT,
    delivered_at TIMESTAMPTZ,
    delivery_error TEXT,
    attempt_count INT NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_outbox_pending
    ON notification_outbox (next_attempt_at)
    WHERE delivered_at IS NULL AND attempt_count < 5;

CREATE INDEX idx_outbox_subscription
    ON notification_outbox (subscription_id, created_at);
