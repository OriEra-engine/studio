Add-Type -AssemblyName System.Drawing
$dir = "$PSScriptRoot\.."
$srcImg = [System.Drawing.Image]::FromFile("$dir\icon-source.jpg")
$w = $srcImg.Width; $h = $srcImg.Height
$srcBmp = New-Object System.Drawing.Bitmap($srcImg)

# emblem bbox (measured): L=82 T=406 R=884 B=1172 -> 803x767, center (483,789)
$ebW = 803; $ebH = 767; $cx = 483; $cy = 789
$fill = 0.82
$side = [int]([Math]::Max($ebW, $ebH) / $fill)
# clamp square crop inside image, keep emblem centered as much as possible
$L = $cx - [int]($side / 2); $T = $cy - [int]($side / 2)
if ($L -lt 0) { $L = 0 }
if ($T -lt 0) { $T = 0 }
if ($L + $side -gt $w) { $L = $w - $side }
if ($T + $side -gt $h) { $T = $h - $side }
Write-Output ("crop: side=$side L=$L T=$T  emblem will fill " + [int]($ebW / $side * 100) + "% x " + [int]($ebH / $side * 100) + "%")

$out = New-Object System.Drawing.Bitmap(1024, 1024)
$g = [System.Drawing.Graphics]::FromImage($out)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
$g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
$srcRect = New-Object System.Drawing.Rectangle($L, $T, $side, $side)
$g.DrawImage($srcBmp, (New-Object System.Drawing.Rectangle(0, 0, 1024, 1024)), $srcRect, [System.Drawing.GraphicsUnit]::Pixel)
$g.Dispose()
$out.Save("$dir\app-icon.png", [System.Drawing.Imaging.ImageFormat]::Png)
Write-Output "app-icon.png saved (1024x1024, zoomed)"
$srcBmp.Dispose(); $srcImg.Dispose(); $out.Dispose()
