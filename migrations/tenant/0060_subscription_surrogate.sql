-- ============================================================================
-- 0060: Give a Subscription a narrow surrogate identity
-- ============================================================================
-- `subscriptions.id` is an externally meaningful NGSI-LD URI (clause 5.2.12)
-- and stays the primary key: the API addresses a Subscription by that URI and
-- nothing here changes it. What changes is what the notification queue carries.
-- A delivery row exists once per matched Subscription per affected Entity, so
-- the queue is the highest-cardinality relation in the schema that references a
-- Subscription. Repeating a variable-length, locale-collated URI in every one of
-- those tuples, in every index on them, and in every hash table that joins them
-- back to `subscriptions` is the single largest avoidable cost on the delivery
-- path. `subscription_key` is the identity those rows carry instead.
--
-- The column is appended rather than placed by alignment. The physical-order
-- rule this schema follows applies to tables that are built or rebuilt; adding
-- an identity column already rewrites the heap, but reaching the ordered layout
-- would mean rebuilding a table that every subscription read and the whole
-- lifecycle path addresses by `id`, for a row count that stays in the thousands
-- and a queue that never scans `subscriptions` sequentially.
--
-- The UNIQUE constraint is not decoration: a foreign key can only point at a
-- unique constraint, and `notification_deliveries` points at this one so that
-- deleting a Subscription reclaims its queued deliveries through ON DELETE
-- CASCADE inside the same statement, with no read-modify-write loop in the
-- broker.
-- ============================================================================

ALTER TABLE subscriptions
    ADD COLUMN subscription_key BIGINT GENERATED ALWAYS AS IDENTITY;

ALTER TABLE subscriptions
    ADD CONSTRAINT subscriptions_subscription_key_key UNIQUE (subscription_key);
