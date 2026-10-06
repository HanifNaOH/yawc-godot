# Yawc Godot Transport

**Author:** HanifNaOH

A Godot 4.7 GDExtension addon that provides the `YawcTransport` WebSocket node. The `.gdextension` descriptor and compiled platform libraries are kept together in this directory for installation under `res://addons/yawc_transport`.

The addon version is maintained in the repository's `Cargo.toml`. Matching `vX.Y.Z` tags are packaged as GitHub Releases after CI succeeds.

This is a runtime GDExtension, not an editor plugin; `yawc_godot.gdextension` is its addon descriptor, so no `plugin.cfg` is required.