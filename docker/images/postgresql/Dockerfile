FROM golang:1.26.1-alpine3.23 AS tools

RUN apk add --no-cache git gcc musl-dev
RUN go install github.com/timescale/timescaledb-tune/cmd/timescaledb-tune@latest
RUN go install github.com/timescale/timescaledb-parallel-copy/cmd/timescaledb-parallel-copy@latest

FROM postgres:18.3-alpine3.23 AS base

RUN apk update
RUN apk add --update icu-data-full musl musl-utils musl-locales tzdata
RUN apk cache clean

FROM base

RUN apk update

RUN apk add --no-cache --virtual .fetch-deps \
    wget \
    ca-certificates \
    openssl \
    git \
    tar

RUN apk add --no-cache --virtual .build-deps \
    build-base \
    make \
    automake \
    autoconf \
    libtool \
    file \
    gcc \
    g++ \
    perl \
    clang19-dev \
    llvm19-dev \
    libxml2-dev \
    json-c-dev \
    geos-dev \
    gdal-dev \
    proj-dev \
    protobuf-c-dev \
    postgresql-dev \
    cmake \
    clang19 \
    llvm19 \
    dpkg \
    dpkg-dev \
    util-linux-dev \
    libc-dev \
    coreutils \
    krb5-dev \
    gettext-dev

RUN apk add --no-cache --virtual .postgis-rundeps \
    json-c \
    geos \
    gdal \
    proj \
    protobuf-c

RUN apk add --no-cache --virtual .timescaledb-rundeps \
    openssl \
    openssl-dev

WORKDIR /usr/src

ENV POSTGIS_VERSION=3.6.2
RUN wget -O postgis.tar.gz "https://github.com/postgis/postgis/archive/$POSTGIS_VERSION.tar.gz"
RUN tar -xzf postgis.tar.gz

WORKDIR /usr/src/postgis-$POSTGIS_VERSION
RUN gettextize
RUN ./autogen.sh
RUN ./configure --enable-lto
RUN make -s -j$(nproc)
RUN make -s install

ENV TIMESCALEDB_VERSION=2.26.1

WORKDIR /usr/src
RUN wget -O timescaledb.tar.gz "https://github.com/timescale/timescaledb/archive/$TIMESCALEDB_VERSION.tar.gz"
RUN tar -xzf timescaledb.tar.gz

WORKDIR /usr/src/timescaledb-$TIMESCALEDB_VERSION
# RUN ./bootstrap -DPROJECT_INSTALL_METHOD="docker" -DAPACHE_ONLY=1 -DREGRESS_CHECKS=OFF
RUN ./bootstrap -DPROJECT_INSTALL_METHOD="docker" -DREGRESS_CHECKS=OFF
RUN cd ./build && make install
RUN cd ../..

WORKDIR /
RUN rm -rf /usr/src
# RUN rm -f $(pg_config --pkglibdir)/timescaledb-tsl-*.so

RUN apk del .fetch-deps .build-deps
RUN apk add --no-cache bash

RUN sed -r -i "s/[#]*\s*(shared_preload_libraries)\s*=\s*'(.*)'/\1 = 'timescaledb,pg_stat_statements,\2'/;s/,'/'/" /usr/local/share/postgresql/postgresql.conf.sample

COPY --from=tools /go/bin/timescaledb-tune /usr/local/bin/timescaledb-tune
COPY --from=tools /go/bin/timescaledb-parallel-copy /usr/local/bin/timescaledb-parallel-copy

COPY docker-entrypoint.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/docker-entrypoint.sh
