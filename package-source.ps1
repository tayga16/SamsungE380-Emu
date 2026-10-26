$ErrorActionPreference = 'Stop'
$archivePath = Join-Path (Split-Path -Parent $PSScriptRoot) 'Samsung-E830-Emulator-source.zip'
$sourceNames = @('src', 'tools', 'docs', 'Cargo.toml', 'Cargo.lock', 'AGENTS.md', 'README.md',
    'LICENSE', 'LICENSE-APACHE', 'LICENSE-MIT', 'NOTICE', 'bootstrap-windows.ps1',
    'build-windows.ps1', 'package-windows.ps1', 'package-source.ps1')
$sourcePaths = $sourceNames | ForEach-Object { Join-Path $PSScriptRoot $_ }
Compress-Archive -LiteralPath $sourcePaths -DestinationPath $archivePath -CompressionLevel Optimal -Force
Get-Item -LiteralPath $archivePath | Select-Object FullName,Length
