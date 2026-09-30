# Running from source

HTMShell currently runs from its Cargo workspace. There is no packaged installation.

## Requirements

- Rust 1.97 or newer, with Cargo
- A C linker and `pkg-config`
- Development files for the system `libdbus-1` library
- Development headers and libraries for `libpipewire-0.3` and `libspa-0.2`
- Clang and `libclang`, used to generate the PipeWire and SPA Rust bindings

These are build-time requirements, including for a CPU-only build. PipeWire is a
host build dependency even when no audio service is used at runtime.

Running native surfaces additionally requires a Wayland compositor with layer
shell. The GPU renderer requires a compatible Vulkan or OpenGL ES driver; a
Wayland session is not needed for manifest validation or ordinary unit tests.

The locked workspace also contains pinned Git dependencies. Cargo must fetch them once before an offline build can succeed.

## Build

From the repository root:

```sh
cargo build --workspace --locked
```

## Validate a manifest

Manifest validation does not connect to Wayland:

```sh
cargo run -p htmshell-live --locked -- \
  manifest examples/static-panel/shell.json --validate-only
```

A valid package reports `manifest_result=valid`.

## Run an example

Run the tracked static panel on the current Wayland display:

```sh
cargo run -p htmshell-live --release --locked -- \
  manifest examples/static-panel/shell.json
```

The process creates one panel and one initially closed overlay for every eligible output. Use the panel button to open the overlay. Exit the process to remove its surfaces and panel reservation.

If no Wayland display is available, startup fails with a Wayland connection error. HTMShell remains experimental and is not a complete desktop shell.

See [create the first shell](first-shell.md) for the package files.
