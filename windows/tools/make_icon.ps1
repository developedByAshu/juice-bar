# Builds windows/assets/juicebar.ico from the shared app icon master, shared/assets/juicebar.png,
# so the Mac and Windows apps carry the same icon. Each size is resized smoothly and stored as PNG.
# Usage: pwsh windows/tools/make_icon.ps1
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$source = Join-Path $root 'shared/assets/juicebar.png'
$out = Join-Path $root 'windows/assets/juicebar.ico'
$sizes = 16, 20, 24, 32, 40, 48, 64, 256

Add-Type -AssemblyName System.Drawing
$master = [System.Drawing.Image]::FromFile($source)
$images = foreach ($size in $sizes) {
    $bitmap = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $graphics.DrawImage($master, 0, 0, $size, $size)
    $stream = New-Object System.IO.MemoryStream
    $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose(); $bitmap.Dispose()
    , $stream.ToArray()
}
$master.Dispose()

# An .ico file: a 6 byte header, a 16 byte entry per image, then the images.
$file = New-Object System.IO.MemoryStream
$writer = New-Object System.IO.BinaryWriter $file
$writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($index = 0; $index -lt $sizes.Count; $index++) {
    $side = if ($sizes[$index] -ge 256) { 0 } else { $sizes[$index] }
    $writer.Write([byte]$side); $writer.Write([byte]$side); $writer.Write([byte]0); $writer.Write([byte]0)
    $writer.Write([uint16]1); $writer.Write([uint16]32)
    $writer.Write([uint32]$images[$index].Length); $writer.Write([uint32]$offset)
    $offset += $images[$index].Length
}
foreach ($image in $images) { $writer.Write($image) }
$writer.Flush()
[System.IO.File]::WriteAllBytes($out, $file.ToArray())
"Wrote $out ($($file.Length) bytes, sizes $($sizes -join ', '))"
