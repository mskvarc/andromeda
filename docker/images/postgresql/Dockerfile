# syntax=docker/dockerfile:1

FROM postgres:18.6-trixie

# The key packagecloud serves for the TimescaleDB repository, checked by
# fingerprint so a key swapped at the source fails the build.
ARG TIMESCALEDB_SIGNER_FINGERPRINT=1005FB68604CE9B8F6879CF759F18EDF47F24417
# The PGDG archive keeps every version PGDG main has dropped, so the pinned
# PostGIS and pg_repack versions stay installable after a newer release.
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl gnupg; \
    key="$(mktemp)"; \
    curl -fsSL https://packagecloud.io/timescale/timescaledb/gpgkey -o "$key"; \
    fingerprint="$(gpg --show-keys --with-colons "$key" | awk -F: '$1 == "fpr" { print $10; exit }')"; \
    if [ "$fingerprint" != "$TIMESCALEDB_SIGNER_FINGERPRINT" ]; then \
        echo "TimescaleDB key fingerprint is '$fingerprint', expected '$TIMESCALEDB_SIGNER_FINGERPRINT'" >&2; \
        exit 1; \
    fi; \
    gpg --dearmor -o /usr/share/keyrings/timescaledb.gpg "$key"; \
    rm -f "$key"; \
    . /etc/os-release; \
    echo "deb [signed-by=/usr/share/keyrings/timescaledb.gpg] https://packagecloud.io/timescale/timescaledb/debian/ ${VERSION_CODENAME} main" \
        > /etc/apt/sources.list.d/timescaledb.list; \
    echo "deb [ signed-by=/usr/local/share/keyrings/postgres.gpg.asc ] https://apt-archive.postgresql.org/pub/repos/apt ${VERSION_CODENAME}-pgdg-archive main" \
        > /etc/apt/sources.list.d/pgdg-archive.list; \
    apt-get purge -y --auto-remove curl gnupg; \
    rm -rf /var/lib/apt/lists/*

# The release's versions. The broker's TimescaleDB floor equals
# TIMESCALEDB_VERSION, and it updates each tenant database's extensions to
# these versions when it opens the tenant. The maintenance controller drives
# pg_repack's SQL interface itself and only on the release line it was
# written for, so pg_repack is pinned like the others. timescaledb-tune, in
# timescaledb-tools, rewrites the server's settings on every start and the
# entrypoint derives more from them, so a full tag keeps its settings only with
# the tuner pinned too. The tsl edition's TimescaleDB Toolkit is pinned for the
# same reason as the extensions: a rebuild of a full tag ships the same one.
ARG TIMESCALEDB_VERSION=2.31.0
ARG POSTGIS_VERSION=3.6.4
ARG PG_REPACK_VERSION=1.5.3
ARG TIMESCALEDB_TUNE_VERSION=0.19.0
ARG TIMESCALEDB_TOOLKIT_VERSION=1.27.0
RUN set -eux; \
    apt-get update; \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        "postgresql-18-postgis-3=${POSTGIS_VERSION}+*" \
        "postgresql-18-postgis-3-scripts=${POSTGIS_VERSION}+*" \
        "postgresql-18-repack=${PG_REPACK_VERSION}-*" \
        "timescaledb-2-postgresql-18=${TIMESCALEDB_VERSION}~*" \
        "timescaledb-2-loader-postgresql-18=${TIMESCALEDB_VERSION}~*" \
        "timescaledb-tools=${TIMESCALEDB_TUNE_VERSION}~*"; \
    rm -rf /var/lib/apt/lists/*

# apache: strip the TSL module and skip the TSL-licensed toolkit. The license
# setting defaults to the TSL one, whose module is then missing, so the edition
# sets it in its own policy file.
ARG TIMESCALEDB_EDITION=tsl
RUN set -eux; \
    case "$TIMESCALEDB_EDITION" in \
        tsl) \
            apt-get update; \
            DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
                "timescaledb-toolkit-postgresql-18=1:${TIMESCALEDB_TOOLKIT_VERSION}~*"; \
            rm -rf /var/lib/apt/lists/* \
            ;; \
        apache) \
            rm -f "$(pg_config --pkglibdir)"/timescaledb-tsl-*.so; \
            mkdir -p /etc/postgresql/andromeda.conf.d; \
            echo "timescaledb.license = 'apache'" > /etc/postgresql/andromeda.conf.d/05-edition.conf \
            ;; \
        *) echo "TIMESCALEDB_EDITION must be 'tsl' or 'apache', got '$TIMESCALEDB_EDITION'" >&2; exit 1 ;; \
    esac

# timescaledb requires preloading.
RUN sed -ri "s/^[#]?\s*(shared_preload_libraries)\s*=\s*'(.*)'/\1 = 'timescaledb,pg_stat_statements,\2'/;s/,'/'/" \
    /usr/share/postgresql/postgresql.conf.sample

COPY andromeda.conf /etc/postgresql/andromeda.conf.d/10-andromeda.conf
COPY andromeda-entrypoint.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/andromeda-entrypoint.sh
# A new ENTRYPOINT clears the CMD the base image declares.
ENTRYPOINT ["andromeda-entrypoint.sh"]
CMD ["postgres"]

# Probes over TCP: the server the entrypoint starts to initialise a new data
# directory listens on no TCP address, so the check passes only once the real
# server accepts connections. pg_isready needs no valid role or database.
HEALTHCHECK --interval=10s --timeout=5s --start-period=2m --start-interval=1s --retries=3 \
    CMD pg_isready -q -h 127.0.0.1 || exit 1

# CI overrides licenses per edition.
LABEL org.opencontainers.image.title="PostgreSQL with PostGIS and TimescaleDB" \
      org.opencontainers.image.description="PostgreSQL 18 with PostGIS 3 and TimescaleDB 2, tuned for high-throughput time-series ingest" \
      org.opencontainers.image.authors="Matej Škvarč <matej@skvarc.com>" \
      org.opencontainers.image.vendor="Matej Škvarč" \
      org.opencontainers.image.base.name="docker.io/library/postgres:18.6-trixie" \
      org.opencontainers.image.licenses="PostgreSQL AND GPL-2.0-or-later AND Apache-2.0 AND LicenseRef-Timescale"
