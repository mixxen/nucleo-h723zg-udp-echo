# MCUboot for NUCLEO-H723ZG

This directory contains the reproducible MCUboot configuration used by the
Rust firmware. Generated dependencies, private keys, and build artifacts are
intentionally not committed.

## Security boundary

`root-ed25519.pem` is the firmware release-signing private key. Never commit,
email, or place a production copy on a CI runner that builds untrusted pull
requests. MCUboot contains only the corresponding public key.

The SSH host and login keys under `Rust/.ssh/` have a different purpose and
must not be reused for firmware signing.

## Flash layout

| Region | Address | Size | Purpose |
| --- | --- | --- | --- |
| MCUboot | `0x08000000` | 128 KiB | Signature verification and image selection |
| Primary slot | `0x08020000` | 384 KiB | Currently running signed image |
| Secondary slot | `0x08080000` | 512 KiB | Swap workspace plus staged update |
| Staged image start | `0x080A0000` | up to 256 KiB | Effective signed-image area in offset-swap format |

MCUboot's offset-swap mode reserves the first sector of the secondary slot.
The network updater must therefore begin a staged image at `0x080A0000`.
The primary slot's final 128 KiB sector holds MCUboot's trailer, so the signed
image is limited to 256 KiB even though more secondary-slot storage exists.

## Generate a development signing key

From the repository root:

```powershell
powershell -ExecutionPolicy Bypass -File .\Bootloader\tools\provision-signing-key.ps1
```

The ignored `root-ed25519.pem` file is the private key. The companion
`root-ed25519.pub.pem` is useful for inspection; MCUboot's build generates the
public-key C data that is compiled into the bootloader.

## Build

After following the Zephyr workspace setup in the main implementation plan:

```powershell
powershell -ExecutionPolicy Bypass -File .\Bootloader\tools\build-mcuboot.ps1 -Pristine
```

The bootloader HEX file is written to `Bootloader\build\zephyr\zephyr.hex`.
