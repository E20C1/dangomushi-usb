# Android USB Storage Contract Spike

This directory is the isolated A0 implementation spike for native Android USB access. It does not change the production DANGOMUSHI Workspace format, desktop UI, M3 mobile handoff, or M6 reconstruction contracts.

The APK uses Android NativeActivity plus eframe/egui and calls Android StorageManager through JNI. It lists removable mounted StorageVolume entries and runs the existing Rust Workspace, session, editor-recovery, File Browser, and Generic Workspace Backup code against a temporary directory on the selected volume.

## Requirements

- Android 11 / API 30 or newer
- ARM64 Android device
- USB OTG storage
- DANGOMUSHI Storage Spike granted Android All files access
- Rust 1.99.1 with the aarch64-linux-android target
- Android SDK and NDK
- cargo-apk 0.10.0 for this spike packaging path

cargo-apk is temporary A0 packaging infrastructure only. The A0 APK is debug-signed so the spike does not introduce or require a production release signing key. Production Android packaging and signing remain later implementation decisions.

## Build

Windows:

    scripts\build-android-storage-spike.cmd

Unix:

    sh scripts/build-android-storage-spike.sh

The scripts intentionally do not install toolchains or mutate the Android SDK. They fail if required external build prerequisites are absent.

A0 pins the direct Android-facing crate versions in Cargo.toml, but it intentionally does not claim a frozen transitive Android dependency graph or a portable Android toolchain. cargo-apk 0.10.0 does not expose Cargo's --locked flag on its build command. Frozen and verified Android build inputs belong to the later Portable Android Build Environment work, not to this storage-contract spike.

## Device procedure

1. Install the generated APK.
2. Grant the app Android All files access in system settings.
3. Attach the USB drive.
4. Press REFRESH ANDROID STORAGE.
5. Select the removable mounted volume.
6. Press RUN STORAGE CONTRACT.
7. Record every PASS or FAIL line.
8. Separately press ARM USB REMOVAL TEST, then physically remove the USB drive. PASS means the probe remains alive and observes the volume becoming unavailable.

The automated contract creates a uniquely named temporary directory directly under the selected volume and performs best-effort cleanup after the run. It never intentionally targets the live DANGOMUSHI/workspace directory.
