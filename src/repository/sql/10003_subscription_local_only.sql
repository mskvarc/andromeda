-- localOnly field for subscriptions (NGSI-LD §5.5.13).
-- When true, the subscription only matches entities stored locally,
-- not those sourced from Context Source Registrations.
ALTER TABLE subscriptions ADD COLUMN IF NOT EXISTS local_only BOOLEAN NOT NULL DEFAULT FALSE;
