-- Transient storage support (NGSI-LD §4.22).
-- expiresAt is a system TemporalProperty indicating when an entity or attribute
-- shall become invalid and may be automatically removed.
ALTER TABLE entities ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_entities_expires_at ON entities(expires_at) WHERE expires_at IS NOT NULL;
