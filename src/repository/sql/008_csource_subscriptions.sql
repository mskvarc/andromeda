-- ============================================================================
-- 008: Context Source Registration Subscriptions
-- ============================================================================
-- NGSI-LD CSR Subscription management (spec clause 5.11).
--
-- A CSR Subscription monitors changes to Context Source Registrations and
-- sends CsourceNotification payloads when matching CSRs are created,
-- updated, or deleted.
--
-- Reuses the same Subscription data type (clause 5.2.12) but matching is
-- performed against CSR information properties rather than entity changes.
-- ============================================================================

CREATE TABLE IF NOT EXISTS csource_subscriptions (
    id TEXT NOT NULL PRIMARY KEY,
    type TEXT NOT NULL DEFAULT 'Subscription',
    subscription_name TEXT,
    description TEXT,
    entities JSONB,
    watched_attributes TEXT[],
    notification_trigger TEXT[],
    time_interval DOUBLE PRECISION,
    q TEXT,
    geo_q JSONB,
    scope_q TEXT,
    csf TEXT,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    notification JSONB NOT NULL,
    expires_at TIMESTAMPTZ,
    throttling DOUBLE PRECISION,
    status TEXT NOT NULL DEFAULT 'active',
    temporal_q JSONB,
    lang TEXT,
    jsonld_context TEXT,
    times_sent INTEGER NOT NULL DEFAULT 0,
    last_notification TIMESTAMPTZ,
    last_success TIMESTAMPTZ,
    last_failure TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_csource_subscriptions_status ON csource_subscriptions(status);
CREATE INDEX IF NOT EXISTS idx_csource_subscriptions_expires_at ON csource_subscriptions(expires_at);
