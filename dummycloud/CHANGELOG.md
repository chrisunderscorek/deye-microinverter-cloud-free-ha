# Changelog

## 1.1.1

- Remove the trailing period from the Home Assistant app description to avoid a duplicated punctuation mark in the app UI.
- Add an explicit changelog for the HAOS app package.

## 1.1.0

- Initial Home Assistant OS app package build for dummycloud.
- Publish multi-architecture images for `aarch64` and `amd64` to GHCR.
- Add Home Assistant UI configuration for MQTT broker URL, optional MQTT credentials, TLS certificate checking, and log level.
- Expose TCP port `10000` for Deye inverter cloud connections.
