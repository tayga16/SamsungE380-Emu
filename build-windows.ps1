param([switch] $Test, [switch] $Check)
$ErrorActionPreference = 'Stop'
$toolRoot = Join-Path $PSScriptRoot '.tools'
$env:RUSTUP_HOME = Join-Path $toolRoot 'rustup'
$env:CARGO_HOME = Join-Path $toolRoot 'cargo'
$compilerRoot = Join-Path $toolRoot 'w64devkit\bin'
$env:PATH = (Join-Path $env:CARGO_HOME 'bin') + ';' + (Join-Path $toolRoot 'cmake-3.31.8-windows-x86_64\bin') + ';C:\Program Files\Git\usr\bin;' + $compilerRoot + ';C:\Users\Administrator\AppData\Local\Programs\Python\Python312;' + $env:PATH
$env:LIBCLANG_PATH = Join-Path $toolRoot 'libclang\libclang-18.1.1.data\platlib\clang\native'
$mingwIncludes = (Join-Path $toolRoot 'w64devkit\include').Replace('\', '/')
$gccIncludes = (Join-Path $toolRoot 'w64devkit\lib\gcc\x86_64-w64-mingw32\16.2.0\include').Replace('\', '/')
$env:BINDGEN_EXTRA_CLANG_ARGS = '--target=x86_64-pc-windows-gnu -I' + $mingwIncludes + ' -I' + $gccIncludes
$env:CMAKE_GENERATOR = 'MinGW Makefiles'
$env:CC = Join-Path $compilerRoot 'gcc.exe'
$env:CXX = Join-Path $compilerRoot 'g++.exe'
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = $env:CC
# Rust ships the matching CRT and unwind archives; w64devkit intentionally has
# no separate libgcc_eh. Tell rustc to use its bundled Windows GNU runtime.
$env:RUSTFLAGS = '-C link-self-contained=yes'
Push-Location $PSScriptRoot
try {
    if (-not (Test-Path -LiteralPath 'Cargo.lock')) {
        cargo generate-lockfile
        if ($LASTEXITCODE -ne 0) { throw 'Dependency resolution failed.' }
    }
    if ($Test) { cargo test --release --locked }
    elseif ($Check) { cargo check --locked --all-targets }
    else { cargo build --release --locked }
    if ($LASTEXITCODE -ne 0) { throw 'Cargo failed.' }
} finally { Pop-Location }
