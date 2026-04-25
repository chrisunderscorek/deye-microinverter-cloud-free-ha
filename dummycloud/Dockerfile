ARG BUILD_FROM=ghcr.io/home-assistant/base:latest
FROM ${BUILD_FROM}

ARG BUILD_ARCH=amd64
ARG BUILD_VERSION=dev

LABEL \
    io.hass.name="Deye Dummycloud" \
    io.hass.description="Local Deye Solarman cloud replacement with MQTT discovery for Home Assistant" \
    io.hass.type="app" \
    io.hass.version="${BUILD_VERSION}" \
    io.hass.arch="${BUILD_ARCH}"

WORKDIR /app

ENV NODE_ENV=production

COPY package.json package-lock.json ./
RUN apk add --no-cache nodejs npm \
    && npm ci --omit=dev \
    && npm cache clean --force

COPY app.js ./
COPY src ./src
COPY run.sh /run.sh

RUN chmod a+x /run.sh

EXPOSE 10000/tcp

CMD ["/run.sh"]
