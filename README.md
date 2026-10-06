# Yawc Godot Transport

A Rust-backed WebSocket transport for Godot 4.7, exposed as the `YawcTransport` GDExtension node. It handles WebSocket connection lifecycle and text messages; Archipelago protocol and game logic remain in GDScript.

**Author:** HanifNaOH

## Platform Support

The checked-in GDExtension descriptor currently declares **Windows x86_64 (GNU)** only. Linux and macOS builds are not configured yet.

## Requirements

- Godot 4.7 or later
- Rust toolchain 1.99.0 (selected by `rust-toolchain.toml`)
- MinGW-w64 x86_64 GCC, required to link the Windows GNU target

## Build and Test

The Cargo configuration defaults to `x86_64-pc-windows-gnu` and puts build output under `.godot/cargo-target`. Copy the library into the addon's `bin` directory so the `.gdextension` descriptor can find it:

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

The addon is packaged under `addons/yawc_transport` with its `.gdextension` descriptor and platform libraries in `bin/`. It is a runtime GDExtension, not an editor plugin, so it does not need a `plugin.cfg`. Open `project.godot` in Godot to use the extension.

## Godot API

Create and add a transport node, then connect to its signals before calling `connect_to_url`:

```gdscript
var transport := YawcTransport.new()
add_child(transport)

transport.opened.connect(_on_opened)
transport.text_message.connect(_on_text_message)
transport.closed.connect(_on_closed)
transport.error.connect(_on_error)

transport.connect_to_url("wss://example.org/socket")
```

`connect_to_url(url)`, `send_text(message)`, and `close()` return whether the command was accepted by the transport worker. Connection results and incoming text arrive through `opened`, `closed`, `error(message)`, and `text_message(message)` signals.

## CI

The test workflow runs formatting and Rust tests on every push and pull request using the single pinned Rust toolchain, 1.99.0. The separate build workflow runs on Windows, builds the Windows x86_64 GNU addon, and runs GUT natively. Manual builds accept an optional version label without a leading `v` (for example, `0.2.0-beta.1`); blank uses the `Cargo.toml` version. Selecting **Publish GitHub Release** publishes that version.

## Versioning and Releases

Set the version manually in `Cargo.toml` under `[package]`:

```toml
[package]
version = "0.1.0"
```

Before a release, update `Cargo.toml` and add the matching `X.Y.Z` section to `CHANGELOG.md`. Run `cargo check` to refresh `Cargo.lock`, then commit and push all three files. Either push a matching `vX.Y.Z` tag, or manually run **Build GDExtension**, optionally enter the matching Cargo version without the leading `v` (blank defaults to Cargo), and select **Publish GitHub Release**. A manual release creates the matching tag at the selected commit. Both paths use the changelog section as release notes and reject version mismatches or missing entries.

```powershell
git tag -a v0.1.0 -m "v0.1.0"
git push origin v0.1.0
```