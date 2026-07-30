//! Converts locally provisioned SSH key files into private firmware constants.
//!
//! The source repository contains no private keys. `tools/provision_ssh.ps1`
//! creates them under the Git-ignored `.ssh` directory. Keeping the conversion
//! here also means application code only has to work with fixed-size byte
//! arrays, which is natural in a `no_std` firmware.

use std::{env, fs, path::Path};

const KEY_BYTES: usize = 32;

fn read_hex_key(path: &Path) -> [u8; KEY_BYTES] {
    let text = fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}); run tools/provision_ssh.ps1 first",
            path.display()
        )
    });
    let text = text.trim();
    assert_eq!(
        text.len(),
        KEY_BYTES * 2,
        "{} must contain exactly {} hexadecimal characters",
        path.display(),
        KEY_BYTES * 2
    );

    let mut bytes = [0; KEY_BYTES];
    for (index, slot) in bytes.iter_mut().enumerate() {
        let start = index * 2;
        *slot = u8::from_str_radix(&text[start..start + 2], 16)
            .unwrap_or_else(|_| panic!("{} contains invalid hexadecimal", path.display()));
    }
    bytes
}

fn main() {
    let manifest = env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR");
    let key_directory = Path::new(&manifest).join(".ssh");
    let host_key_path = key_directory.join("host_ed25519.seed");
    let authorized_key_path = key_directory.join("authorized_ed25519.hex");

    println!("cargo:rerun-if-changed={}", host_key_path.display());
    println!("cargo:rerun-if-changed={}", authorized_key_path.display());

    // Host-only tests do not compile the firmware or use SSH secrets. Supplying
    // placeholders here keeps those tests runnable from a fresh clone before
    // the developer provisions keys.
    let firmware_enabled = env::var_os("CARGO_FEATURE_FIRMWARE").is_some();
    let (host_key, authorized_key) = if firmware_enabled {
        (
            read_hex_key(&host_key_path),
            read_hex_key(&authorized_key_path),
        )
    } else {
        ([0; KEY_BYTES], [0; KEY_BYTES])
    };

    let generated = format!(
        "pub const SSH_HOST_KEY_SEED: [u8; 32] = {host_key:?};\n\
         pub const SSH_AUTHORIZED_KEY: [u8; 32] = {authorized_key:?};\n"
    );
    let output =
        Path::new(&env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("ssh_keys.rs");
    fs::write(output, generated).expect("write generated SSH constants");
}
