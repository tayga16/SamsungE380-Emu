$ErrorActionPreference = 'Stop'
$packageRoot = Join-Path $PSScriptRoot 'dist'
New-Item -ItemType Directory -Path $packageRoot -Force | Out-Null
foreach ($binaryName in @('e830-emulator.exe', 'e830-cli.exe')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('target\release\' + $binaryName)) -Destination (Join-Path $packageRoot $binaryName) -Force
}
foreach ($documentName in @('README.md', 'LICENSE', 'LICENSE-APACHE', 'LICENSE-MIT', 'NOTICE')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $documentName) -Destination (Join-Path $packageRoot $documentName) -Force
}
$firmwareTarget = Join-Path $packageRoot 'firmware'
New-Item -ItemType Directory -Path $firmwareTarget -Force | Out-Null
Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'firmware') -File | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $firmwareTarget $_.Name) -Force
}
Get-ChildItem -LiteralPath $packageRoot -File | Select-Object Name,Length
