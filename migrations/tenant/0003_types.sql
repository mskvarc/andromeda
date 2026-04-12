-- ============================================================================
-- 002: Custom Enum Types
-- ============================================================================
-- PostgreSQL enum types used throughout the schema. Defined before table
-- migrations so columns can reference them directly.
--
-- Enums provide type-safe validation at the database level, are more
-- storage-efficient than TEXT with CHECK constraints (4 bytes vs variable
-- length), and are self-documenting via \dT+.
-- ============================================================================

-- JSON-LD @context document kind (spec clause 5.2.19)
CREATE TYPE jsonld_context_kind AS ENUM (
    'Cached',
    'Hosted',
    'ImplicitlyCreated'
);

-- Subscription discriminator: entity subscriptions vs CSR subscriptions
CREATE TYPE subscription_kind AS ENUM (
    'entity',
    'csource'
);

-- Subscription lifecycle state (spec clause 5.2.12)
CREATE TYPE subscription_status AS ENUM (
    'active',
    'paused',
    'expired'
);

-- CSR distributed operation mode (spec clause 4.3.6)
CREATE TYPE csr_mode AS ENUM (
    'inclusive',
    'exclusive',
    'redirect',
    'auxiliary'
);

-- CSR health status
CREATE TYPE csr_status AS ENUM (
    'ok',
    'failed'
);

-- Snapshot lifecycle state (spec clause 5.2.22)
CREATE TYPE snapshot_status AS ENUM (
    'preparing',
    'success',
    'partial',
    'empty',
    'failure'
);
