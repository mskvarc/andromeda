-- ============================================================================
-- 0025: Subscription splitEntities
-- ============================================================================
-- Table 5.2.12-1 gives a Subscription an optional `splitEntities` member: when
-- true, single entities are assumed to be distributed across context sources,
-- so the filters of the subscription can only be applied after the entity has
-- been re-assembled (clause 5.8.6). The member is tri-state on the wire — an
-- absent value defers to the deployment default — so the column is nullable
-- rather than defaulted.
-- ============================================================================

ALTER TABLE subscriptions
    ADD COLUMN split_entities BOOLEAN;
