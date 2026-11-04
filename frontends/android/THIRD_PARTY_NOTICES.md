# Third-party notices

## PocketHLE Android library UI

The Android library card layout, launcher-style grid, theme, selected vector drawables and NDK linker-support pattern are adapted from [j92580498-max/PocketHLE](https://github.com/j92580498-max/PocketHLE), commit `25d13039d8f0ad3755b441215745a53e382edb1b`. PocketHLE is dual-licensed under Apache-2.0 or MIT; the corresponding license texts are in `third_party/PocketHLE/` and in the APK at `app/src/main/assets/licenses/PocketHLE/`.

## Unicorn Engine

The Android native library statically links `unicorn-engine` / `unicorn-engine-sys` version 2.1.5, which is licensed under GPL-2.0 and includes QEMU-derived code. The GPL-2.0 license text is included in the APK at `app/src/main/assets/licenses/GPL-2.0-Unicorn.txt`. The corresponding source is the pinned crate source identified by `Cargo.lock` and available from [crates.io](https://crates.io/crates/unicorn-engine-sys/2.1.5).

## Android libraries

AndroidX, Material Components and Kotlin libraries are used under their respective upstream licenses. Their exact versions are pinned in the Gradle dependency declarations.
