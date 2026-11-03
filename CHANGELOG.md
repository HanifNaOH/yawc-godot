# Changelog

Notable changes to Yawc Godot Transport are recorded here.

## [Unreleased]

### Added

- Binary WebSocket send and receive support in `YawcTransport`.

## [0.2.0] - 2026-11-01

### Changed

- Bound inbound WebSocket payloads and validate text frames; compression is disabled until decompressed payload sizes can be limited.
- Detach the worker during node teardown so waiting for a peer's close reply does not block the Godot thread.

## [0.1.0] - 2026-10-06

### Added

- Initial Godot 4.7 GDExtension exposing the `YawcTransport` node.
- WebSocket text transport with `ws` and `wss`, lifecycle signals, and balanced compression.
- Background transport worker with connection timeout and main-thread signal delivery.
