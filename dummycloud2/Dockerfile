ARG BUILD_FROM=ghcr.io/home-assistant/base:latest

FROM rust:1.95-alpine AS builder

WORKDIR /build

RUN apk add --no-cache binutils build-base openssl-dev perl pkgconf

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --locked \
    && strip target/release/deye-dummycloud

FROM ${BUILD_FROM}

ARG BUILD_ARCH=amd64
ARG BUILD_VERSION=dev

LABEL \
    io.hass.name="Deye Dummycloud v2" \
    io.hass.description="Rust reimplementation of the local Deye Solarman cloud replacement" \
    io.hass.type="app" \
    io.hass.version="${BUILD_VERSION}" \
    io.hass.arch="${BUILD_ARCH}"

WORKDIR /app

RUN apk add --no-cache ca-certificates libgcc openssl

COPY --from=builder /build/target/release/deye-dummycloud /usr/local/bin/deye-dummycloud
COPY run.sh /run.sh

RUN chmod a+x /run.sh

EXPOSE 10000/tcp

CMD ["/run.sh"]
