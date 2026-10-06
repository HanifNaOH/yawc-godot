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

GitHub Actions checks formatting, runs the Rust and GUT unit tests, checks the echo-server example, and packages the Windows x86_64 GNU addon. Pushing a matching `vX.Y.Z` tag creates a GitHub Release with the addon ZIP after CI passes.

## Versioning and Releases

Set the version manually in `Cargo.toml` under `[package]`:

```toml
[package]
version = "0.1.0"
```

After changing it, run `cargo check` to refresh `Cargo.lock`, then commit and push both files. Push a matching `vX.Y.Z` tag to trigger the release; the workflow rejects tags that do not match the Cargo version.

```powershell
git tag -a v0.1.0 -m "v0.1.0"
git push origin v0.1.0
```