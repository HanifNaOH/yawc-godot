# Yawc Godot Backend

Yawc Godot Backend is a small Rust GDExtension for Godot 4.7 and later. It exposes yawc as a `YawcTransport` node with text and binary WebSocket messages.

This repository contains the backend addon and its test harness, not a game, UI, or separate WebSocket server. The extension runs inside a host Godot project; that project is responsible for its UI and any application-specific protocol or game logic. Network I/O runs on a worker thread, while results are delivered to Godot through signals.

**Author:** HanifNaOH

## Use in a Godot Project

Download a release archive and extract `addons/yawc_transport` into the host project's `res://addons/` directory. The addon includes its `.gdextension` descriptor and platform libraries. It is a runtime extension, not an editor plugin, so no `plugin.cfg` or editor-plugin activation is needed.

Create the node, connect its signals, then connect and send messages:

```gdscript
var transport: Node

func _ready() -> void:
	transport = ClassDB.instantiate("YawcTransport") as Node
	add_child(transport)

	transport.opened.connect(_on_opened)
	transport.text_message.connect(_on_text_message)
	transport.binary_message.connect(_on_binary_message)
	transport.closed.connect(_on_closed)
	transport.error.connect(_on_error)

	transport.connect_to_url("wss://example.org/socket")

func _on_opened() -> void:
	transport.send_text("hello")
	transport.send_binary(PackedByteArray([1, 2, 3]))

func _on_text_message(message: String) -> void:
	print(message)

func _on_binary_message(message: PackedByteArray) -> void:
	print(message)

func _on_closed() -> void:
	print("WebSocket closed")

func _on_error(message: String) -> void:
	push_error(message)
```

`connect_to_url(url)`, `send_text(message)`, `send_binary(bytes)`, and `close()` return whether the command was accepted by the worker, not whether the operation succeeded. Observe `opened`, `closed`, `error(message)`, `text_message(message)`, and `binary_message(bytes)` for results. Incoming and outgoing work uses yawc with bounded inbound messages; signals are emitted on Godot's main thread.

## Roadmap

These are possible additions to the Godot client API, not commitments to a release schedule. The goal is to expose yawc features when they are useful to Godot projects while keeping the default node small.

| Feature | Status | Notes |
| --- | --- | --- |
| Configurable yawc options | Candidate | Message limits, UTF-8 validation, TCP no-delay, fragmentation, and backpressure. |
| Handshake headers and subprotocols | Candidate | Allow applications to provide custom request headers and WebSocket subprotocols. |
| Control frames and close details | Candidate | Optional Ping sending, Ping/Pong signals, and close codes/reasons. Yawc already replies to incoming Ping frames. |
| Per-message compression | Deferred | Yawc supports compression, but enabling it requires a safe bound on decompressed input size. |
| Proxy and custom connection setup | Optional | SOCKS5, TCP address overrides, and custom TLS connectors for projects that need them. |
| HTTP/2 WebSockets | Optional | RFC 8441 support requires yawc's optional `http2` feature and a compatible server. |
| Streaming and manual frame control | Separate API | Consider only if Godot projects need large-message streaming or low-level frame access. |

Server-side WebSocket upgrades and alternate runtime integrations are outside this outbound Godot client’s scope.

## Platform Support

The packaged GDExtension currently supports **Windows x86_64 (GNU)** only. Linux and macOS builds are not configured yet.

## Build and Test

To build from source, install Godot 4.7 or later, the Rust toolchain 1.99.0 selected by `rust-toolchain.toml`, and MinGW-w64 x86_64 GCC. Cargo targets `x86_64-pc-windows-gnu` by default and stores build output under `.godot/cargo-target`. Stage the generated libraries where the extension descriptor expects them:

```powershell
cargo build
New-Item -ItemType Directory -Force addons/yawc_transport/bin | Out-Null
Copy-Item .godot/cargo-target/x86_64-pc-windows-gnu/debug/yawc_godot.dll addons/yawc_transport/bin/yawc_godot.windows.debug.x86_64.dll

cargo build --release
Copy-Item .godot/cargo-target/x86_64-pc-windows-gnu/release/yawc_godot.dll addons/yawc_transport/bin/yawc_godot.windows.release.x86_64.dll

cargo test
cargo fmt --all -- --check
godot --headless --path . -s addons/gut/gut_cmdln.gd
```

## CI

The test workflow runs formatting and Rust tests on every push and pull request using the pinned Rust toolchain. The build workflow builds and tests the Windows x86_64 GNU addon. Manual builds accept an optional version label without a leading `v` (for example, `0.2.0-beta.1`); blank uses the `Cargo.toml` version. Selecting **Publish GitHub Release** publishes that version.

## Versioning and Releases

Set the version in `Cargo.toml` under `[package]`:

```toml
[package]
version = "0.2.0"
```

Before a release, update `Cargo.toml` and add the matching `X.Y.Z` section to `CHANGELOG.md`. Run `cargo check` to refresh `Cargo.lock`, then commit and push all three files. Either push a matching `vX.Y.Z` tag, or manually run **Build GDExtension**, optionally enter the matching Cargo version without the leading `v` (blank defaults to Cargo), and select **Publish GitHub Release**. A manual release creates the matching tag at the selected commit. Both paths use the changelog section as release notes and reject version mismatches or missing entries.

```powershell
git tag -a v0.2.0 -m "v0.2.0"
git push origin v0.2.0
```
