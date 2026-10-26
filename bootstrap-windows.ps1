$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$toolRoot = Join-Path $PSScriptRoot '.tools'
$downloadRoot = Join-Path $toolRoot 'downloads'
New-Item -ItemType Directory -Path $downloadRoot -Force | Out-Null

function Get-Download([string] $Url, [string] $Name) {
    $target = Join-Path $downloadRoot $Name
    if (-not (Test-Path -LiteralPath $target)) {
        Write-Output ('Downloading ' + $Name)
        $client = [System.Net.WebClient]::new()
        try { $client.DownloadFile($Url, $target) } finally { $client.Dispose() }
    }
    return $target
}

$devkitPath = Join-Path $toolRoot 'w64devkit\bin\gcc.exe'
if (-not (Test-Path -LiteralPath $devkitPath)) {
    $archivePath = Get-Download 'https://github.com/skeeto/w64devkit/releases/download/v2.10.1/w64devkit-x64-2.10.1.7z.exe' 'w64devkit-x64-2.10.1.7z.exe' | Select-Object -Last 1
    & 'C:\Program Files\7-Zip\7z.exe' x $archivePath ('-o' + $toolRoot) -y -bso0
    if ($LASTEXITCODE -ne 0) { throw 'C/C++ compiler extraction failed.' }
}

$cmakeRoot = Join-Path $toolRoot 'cmake-3.31.8-windows-x86_64'
if (-not (Test-Path -LiteralPath (Join-Path $cmakeRoot 'bin\cmake.exe'))) {
    $archivePath = Get-Download 'https://github.com/Kitware/CMake/releases/download/v3.31.8/cmake-3.31.8-windows-x86_64.zip' 'cmake-3.31.8-windows-x86_64.zip' | Select-Object -Last 1
    $hashPath = Get-Download 'https://github.com/Kitware/CMake/releases/download/v3.31.8/cmake-3.31.8-SHA-256.txt' 'cmake-3.31.8-SHA-256.txt' | Select-Object -Last 1
    $expectedHash = (Get-Content -LiteralPath $hashPath | Where-Object { $_ -match 'cmake-3.31.8-windows-x86_64.zip$' }).Split(' ')[0]
    if ((Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash -ne $expectedHash) { throw 'CMake SHA256 mismatch.' }
    & 'C:\Program Files\7-Zip\7z.exe' x $archivePath ('-o' + $toolRoot) -y -bso0
    if ($LASTEXITCODE -ne 0) { throw 'CMake extraction failed.' }
}

$clangRoot = Join-Path $toolRoot 'libclang'
if (-not (Test-Path -LiteralPath (Join-Path $clangRoot 'libclang-18.1.1.data\platlib\clang\native\libclang.dll'))) {
    $wheelInfo = Invoke-RestMethod -Uri 'https://pypi.org/pypi/libclang/18.1.1/json'
    $wheel = $wheelInfo.urls | Where-Object { $_.filename -match 'win_amd64.whl$' } | Select-Object -First 1
    if ($null -eq $wheel) { throw 'Windows libclang wheel was not found.' }
    $archivePath = Get-Download $wheel.url $wheel.filename | Select-Object -Last 1
    if ((Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash -ne $wheel.digests.sha256) { throw 'libclang SHA256 mismatch.' }
    & 'C:\Program Files\7-Zip\7z.exe' x $archivePath ('-o' + $clangRoot) -y -bso0
    if ($LASTEXITCODE -ne 0) { throw 'libclang extraction failed.' }
}

$env:RUSTUP_HOME = Join-Path $toolRoot 'rustup'
$env:CARGO_HOME = Join-Path $toolRoot 'cargo'
$env:PATH = (Join-Path $toolRoot 'w64devkit\bin') + ';' + $env:PATH
if (-not (Test-Path -LiteralPath (Join-Path $env:CARGO_HOME 'bin\rustc.exe'))) {
    $installerPath = Get-Download 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-gnu/rustup-init.exe' 'rustup-init.exe' | Select-Object -Last 1
    & $installerPath -y --no-modify-path --profile minimal --default-host x86_64-pc-windows-gnu --default-toolchain stable
    if ($LASTEXITCODE -ne 0) { throw 'Rust toolchain installation failed.' }
}
& (Join-Path $env:CARGO_HOME 'bin\rustc.exe') --version
Write-Output 'Local Windows build tools are ready.'
