# RustOS POC

RustOS POC is a minimal Rust-based bootable environment for VMware. It builds a standalone `x86_64-unknown-uefi` binary and places it at `EFI/BOOT/BOOTX64.EFI` inside a GPT/FAT32 boot disk image.

## Current Features

- Standalone Rust UEFI app using `no_std`.
- Text-mode desktop UI.
- Minimal window management:
  - `Tab` switches focus between apps.
  - `Esc` maximizes/restores the focused app.
- Terminal app with built-in commands:
  - `help`
  - `clear`
  - `ls`
  - `pwd`
  - `cd`
  - `cat`
  - `echo`
  - `win`
  - `about`
- File browser app:
  - Arrow keys move/open.
  - `Enter` opens directories.
  - `Backspace` goes up.
  - Text file preview.
- Static in-memory filesystem shared by the terminal and file browser.

## Build

From PowerShell:

```powershell
.\scripts\build.ps1
```

Outputs:

- `dist/rustos-poc.img`: raw GPT/FAT32 UEFI boot disk.
- `dist/rustos-poc.vmdk`: VMware disk, generated when `qemu-img` is installed.
- `dist/rustos-poc.vmx`: ready-to-open VMware VM config, generated with the VMDK.

## Boot In VMware

Open this file in VMware Workstation/Player:

```text
dist/rustos-poc.vmx
```

The VMX uses UEFI firmware, disables Secure Boot, and attaches `rustos-poc.vmdk` as the boot disk.

If the VMDK was not generated, install QEMU or convert `dist/rustos-poc.img` to VMDK with:

```powershell
qemu-img convert -f raw -O vmdk dist\rustos-poc.img dist\rustos-poc.vmdk
```

## Scope

This is intentionally a POC. It runs directly under UEFI firmware and demonstrates the UI/app model before adding lower-level kernel features like interrupts, virtual memory, block drivers, framebuffer graphics, process isolation, or a real filesystem driver.
