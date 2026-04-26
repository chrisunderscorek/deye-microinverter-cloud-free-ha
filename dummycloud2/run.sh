#!/usr/bin/with-contenv bashio
# shellcheck shell=bash
set -euo pipefail

if [[ -f /data/options.json ]]; then
    export LOGLEVEL
    LOGLEVEL="$(bashio::config 'LOGLEVEL')"

    export MQTT_BROKER_URL
    MQTT_BROKER_URL="$(bashio::config 'MQTT_BROKER_URL')"

    export MQTT_CHECK_CERT
    MQTT_CHECK_CERT="$(bashio::config 'MQTT_CHECK_CERT')"

    if bashio::config.has_value 'MQTT_USERNAME'; then
        export MQTT_USERNAME
        MQTT_USERNAME="$(bashio::config 'MQTT_USERNAME')"
    else
        unset MQTT_USERNAME
    fi

    if bashio::config.has_value 'MQTT_PASSWORD'; then
        export MQTT_PASSWORD
        MQTT_PASSWORD="$(bashio::config 'MQTT_PASSWORD')"
    else
        unset MQTT_PASSWORD
    fi
else
    export LOGLEVEL="${LOGLEVEL:-info}"
    export MQTT_CHECK_CERT="${MQTT_CHECK_CERT:-true}"
fi

if [[ -z "${MQTT_BROKER_URL:-}" ]]; then
    bashio::log.fatal "MQTT_BROKER_URL must be configured before starting Deye Dummycloud v2."
fi

if [[ -n "${MQTT_PASSWORD:-}" && -z "${MQTT_USERNAME:-}" ]]; then
    bashio::log.fatal "MQTT_USERNAME must be configured when MQTT_PASSWORD is configured."
fi

exec /usr/local/bin/deye-dummycloud
