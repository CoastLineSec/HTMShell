// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

use std::env;
use std::path::PathBuf;

fn main() {
    let libs = system_deps::Config::new()
        .probe()
        .expect("Cannot find libpipewire");
    let libpipewire = libs.get_by_name("libpipewire").unwrap();
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());

    println!("cargo:rerun-if-changed=wrapper.h");

    let builder = bindgen::Builder::default()
        .header("wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .size_t_is_usize(true)
        .allowlist_function("pw_.*")
        .allowlist_type("pw_.*")
        .allowlist_var("pw_.*")
        .allowlist_var("PW_.*")
        .blocklist_function("spa_.*")
        .blocklist_type("spa_.*")
        .blocklist_item("spa_.*")
        .raw_line("use spa_sys::*;")
        .clang_macro_fallback()
        .clang_macro_fallback_build_dir(&out_path);

    let builder = libpipewire
        .include_paths
        .iter()
        .fold(builder, |builder, path| {
            builder.clang_arg(format!("-I{}", path.to_string_lossy()))
        });

    let bindings = builder.generate().expect("Unable to generate bindings");
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
