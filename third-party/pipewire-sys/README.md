# pipewire-sys

This is the `pipewire-sys` 0.10.0 source used by HTMShell.

The build script has one local compatibility fix. Bindgen macro-fallback
artifacts are written to the crate's Cargo output directory so parallel sys
crate builds cannot overwrite one another.

The crate remains licensed under the included MIT license.
