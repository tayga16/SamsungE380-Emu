# Android arm64-v8a toolchain wrapper for cmake-rs cross-compiles.
#
# Why this file exists
# --------------------
# `unicorn-engine-sys` 2.1.5 invokes CMake via the `cmake` Rust crate. When
# cross-compiling to `aarch64-linux-android`, cmake-rs sets `CMAKE_SYSTEM_NAME`
# but does NOT set `ANDROID_ABI`, and it picks the NDK's bare `clang`
# (not the per-ABI wrapper such as `aarch64-linux-android21-clang`) as
# `CMAKE_C_COMPILER`. As a result, unicorn's `CMakeLists.txt` falls into its
# host-detect branch, runs `clang -dM -E` against the build host's default
# target, sees `__x86_64__`, and configures the i386 TCG JIT — which then
# fails to compile because NDK clang has no x86 cpuid intrinsics.
#
# This wrapper forces `ANDROID_ABI=arm64-v8a`, forwards to the official NDK
# `android.toolchain.cmake`, and is selected per-target via the
# `CMAKE_TOOLCHAIN_FILE_aarch64-linux-android` env var (set in
# `.cargo/config.toml`). With `ANDROID_ABI` set, unicorn's `CMakeLists.txt`
# takes its `elseif(ANDROID_ABI)` branch, sets `UNICORN_TARGET_ARCH=aarch64`,
# and the build picks `qemu/tcg/aarch64/` instead of `qemu/tcg/i386/`.

set(ANDROID_ABI arm64-v8a)
if(NOT DEFINED ANDROID_PLATFORM)
    set(ANDROID_PLATFORM android-24)
endif()

if(NOT DEFINED ENV{ANDROID_NDK_HOME} AND NOT DEFINED ENV{ANDROID_NDK_ROOT})
    message(FATAL_ERROR
        "Set ANDROID_NDK_HOME (or ANDROID_NDK_ROOT) before invoking cargo. "
        "Required for the NDK CMake toolchain include below.")
endif()

if(DEFINED ENV{ANDROID_NDK_HOME})
    set(_e830_ndk_path "$ENV{ANDROID_NDK_HOME}")
else()
    set(_e830_ndk_path "$ENV{ANDROID_NDK_ROOT}")
endif()

include("${_e830_ndk_path}/build/cmake/android.toolchain.cmake")
