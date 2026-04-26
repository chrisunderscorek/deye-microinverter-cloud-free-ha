# Changelog

## 1.1.5

- Recognize Solarman `REPORT` packets in the legacy Node.js protocol parser.
- Keep the legacy app documentation distinct from the Rust v2 app.

## 1.1.4

- Add a custom AppArmor profile for the Home Assistant app package.
- Keep TCP networking available for inverter connections on port `10000` and MQTT or MQTT over TLS broker access.
- Document the published Home Assistant image architectures.

## 1.1.3

- Add inverter cloud server setup guidance to the Home Assistant app documentation.
- Keep the Home Assistant app description concise.

## 1.1.2

- Add Home Assistant app logo and icon assets for the Deye Dummycloud store entry.

## 1.1.1

- Remove the trailing period from the Home Assistant app description to avoid a duplicated punctuation mark in the app UI.
- Add an explicit changelog for the HAOS app package.

## 1.1.0

- Initial Home Assistant OS app package build for dummycloud.
- Publish multi-architecture images for `aarch64` and `amd64` to GHCR.
- Add Home Assistant UI configuration for MQTT broker URL, optional MQTT credentials, TLS certificate checking, and log level.
- Expose TCP port `10000` for Deye inverter cloud connections.
