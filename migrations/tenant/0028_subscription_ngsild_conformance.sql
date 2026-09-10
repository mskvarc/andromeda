-- ============================================================================
-- 0028: Subscription ngsildConformance
-- ============================================================================
-- Table 5.2.12-1 gives a Subscription an optional `ngsildConformance` member:
-- "a semantically versioned string in the form major.minor, which conforms to a
-- version of the NGSI-LD specification". When it is set, clause 5.8.6 has the
-- notification undergo the clause 4.3.6.8 backwards compatibility operation
-- before it is sent.
--
-- The column holds the `major.minor` text the client wrote, because that is the
-- form Table 5.2.12-1 defines and the form a retrieve has to echo; the two
-- components are never compared in SQL, only in the broker, so no composite or
-- numeric encoding would buy anything. It is nullable because the member is
-- optional and an absent member means no amendment at all -- there is no
-- default version to fall back to.
--
-- Both subscription families carry the column. The payload of
-- `/csourceSubscriptions` is the same Subscription datatype (clause 5.11.2.3),
-- so the member is admitted and echoed there too; clause 5.11.7 replaces the
-- clause 5.8.6 send list with its own, which names no amendment, and a
-- CsourceNotification's `data` holds Context Source Registrations rather than
-- the Entity data Tables 4.3.6.8-1 to -3 describe, so it has no effect on a
-- csource notification.
-- ============================================================================

ALTER TABLE subscriptions
    ADD COLUMN ngsild_conformance TEXT;
