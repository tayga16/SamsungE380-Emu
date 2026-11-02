param(
    [Parameter(Mandatory=$true)][string] $FramePath,
    [Parameter(Mandatory=$true)][string] $OutputPath,
    [string] $Language = 'ru-RU'
)
# Local OCR is diagnostic text evidence, not visual verification of a menu.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$frame = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $FramePath).Path)
$header = [System.Text.Encoding]::ASCII.GetString($frame, 0, [Math]::Min(80, $frame.Length))
$match = [regex]::Match($header, '\AP6\n(\d+) (\d+)\n255\n')
if (-not $match.Success) { throw 'Expected emulator P6 RGB frame.' }
$width = [int]$match.Groups[1].Value
$height = [int]$match.Groups[2].Value
if ($width -ne 176 -or $height -ne 220 -or $frame.Length -ne $match.Length + $width * $height * 3) {
    throw 'Expected complete176x220 emulator LCD frame.'
}
$destination = [System.IO.Path]::GetFullPath($OutputPath)
if (Test-Path -LiteralPath $destination) { throw 'Output image already exists; choose a fresh name.' }
$bitmap = [System.Drawing.Bitmap]::new($width, $height)
$scaled = [System.Drawing.Bitmap]::new($width * 4, $height * 4)
try {
    $offset = $match.Length
    for ($y = 0; $y -lt $height; $y++) {
        for ($x = 0; $x -lt $width; $x++) {
            $bitmap.SetPixel($x, $y, [System.Drawing.Color]::FromArgb($frame[$offset], $frame[$offset+1], $frame[$offset+2]))
            $offset += 3
        }
    }
    $graphics = [System.Drawing.Graphics]::FromImage($scaled)
    try {
        $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
        $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
        $graphics.DrawImage($bitmap, 0, 0, $scaled.Width, $scaled.Height)
        $scaled.Save($destination, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose() }
} finally { $scaled.Dispose(); $bitmap.Dispose() }
[void][Windows.Storage.StorageFile,Windows.Storage,ContentType=WindowsRuntime]
[void][Windows.Graphics.Imaging.BitmapDecoder,Windows.Graphics.Imaging,ContentType=WindowsRuntime]
[void][Windows.Media.Ocr.OcrEngine,Windows.Foundation,ContentType=WindowsRuntime]
[void][Windows.Globalization.Language,Windows.Globalization,ContentType=WindowsRuntime]
$asTask = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.IsGenericMethod -and $_.GetParameters().Count -eq 1 -and
    $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
} | Select-Object -First 1
function Wait-WinRT($Operation, [Type] $ResultType) {
    $task = $asTask.MakeGenericMethod($ResultType).Invoke($null, @($Operation))
    $task.Wait()
    return $task.Result
}
$file = Wait-WinRT ([Windows.Storage.StorageFile]::GetFileFromPathAsync($destination)) ([Windows.Storage.StorageFile])
$stream = Wait-WinRT ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
try {
    $decoder = Wait-WinRT ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
    $softwareBitmap = Wait-WinRT ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
    try {
        $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage([Windows.Globalization.Language]::new($Language))
        if ($null -eq $engine) { throw ('OCR language unavailable: ' + $Language) }
        $result = Wait-WinRT ($engine.RecognizeAsync($softwareBitmap)) ([Windows.Media.Ocr.OcrResult])
        Write-Output ('Local OCR (' + $Language + '): ' + $result.Text)
        Write-Output ('Scaled LCD: ' + $destination)
    } finally { $softwareBitmap.Dispose() }
} finally { $stream.Dispose() }
