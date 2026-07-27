//! Cargo build script for the firmware.
//!
//! Unlike the firmware under `src/`, a build script runs on the Windows host
//! while Cargo is compiling. It can therefore use `std`, files, and environment
//! variables. Its job here is to make the custom memory map available to the
//! embedded linker.

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    // Cargo gives each build a private output directory through `OUT_DIR`.
    // Copy the checked-in linker memory map there so the linker can discover
    // it without depending on the caller's current working directory.
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    File::create(out.join("memory.x"))
        .expect("create memory.x")
        .write_all(include_bytes!("memory.x"))
        .expect("write memory.x");

    // Lines prefixed with `cargo:` are instructions consumed by Cargo:
    // - add OUT_DIR to the linker's file search path;
    // - rerun this script if memory.x changes;
    // - avoid large page-alignment gaps with `--nmagic`;
    // - use cortex-m-rt's `link.x` layout; and
    // - include defmt's logging metadata layout.
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
}
