-- ============================================================================
-- 0029: Subscription notificationTrigger is optional
-- ============================================================================
-- Table 5.2.12-1 gives `notificationTrigger` cardinality 0..1 and says "If not
-- present, the default is the combination 'attributeCreated' and
-- 'attributeUpdated'". That is a default for *matching*, not a value the
-- Context Subscriber wrote: the column materialised it at insertion, so an
-- absent member became a present one and every Subscription read back a
-- selection its writer never made.
--
-- It matters for the two families differently. A Subscription of clause 5.8 is
-- matched under the default and serves it back, so clause 5.2.12's sentence is
-- satisfied by supplying it when the resource is rendered. A Context Source
-- Registration Subscription is not: clause 5.11.2.4 notifies "whenever there is
-- a change of a matching Context Source Registration (creation, update,
-- deletion)" and reports which in the clause 5.3.2 `triggerReason`, with no
-- notification trigger selecting anything, so a default there asserts a
-- selection the operation does not make.
--
-- The column therefore records what was written and nothing else. NULL is the
-- absent member, which `effective_notification_triggers` reads as the clause
-- 5.2.12 default wherever a match is decided.
-- ============================================================================

ALTER TABLE subscriptions
    ALTER COLUMN notification_trigger DROP NOT NULL,
    ALTER COLUMN notification_trigger DROP DEFAULT;
