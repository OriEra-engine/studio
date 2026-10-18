Add-Type -AssemblyName System.Drawing
$inPath = "C:\Users\Lenovo\.zcode\workspace\default\oriera-studio\icon-source.jpg"
$outPath = "C:\Users\Lenovo\.zcode\workspace\default\oriera-studio\app-icon.png"
$srcImg = [System.Drawing.Image]::FromFile($inPath)
$w = $srcImg.Width; $h = $srcImg.Height
$srcBmp = New-Object System.Drawing.Bitmap($srcImg)
Write-Output ("source: " + $w + "x" + $h)

$side = [Math]::Max($w, $h)
$sq = New-Object System.Drawing.Bitmap($side, $side)
$g = [System.Drawing.Graphics]::FromImage($sq)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
$g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
if ($w -lt $h) {
  $pad = [int](($h - $w) / 2)
  $g.DrawImage($srcBmp, (New-Object System.Drawing.Rectangle(0, 0, $pad, $h)), (New-Object System.Drawing.Rectangle(0, 0, 1, $h)), [System.Drawing.GraphicsUnit]::Pixel)
  $g.DrawImage($srcBmp, (New-Object System.Drawing.Rectangle(($side - $pad), 0, $pad, $h)), (New-Object System.Drawing.Rectangle(($w - 1), 0, 1, $h)), [System.Drawing.GraphicsUnit]::Pixel)
  $g.DrawImage($srcBmp, $pad, 0, $w, $h)
} else {
  $pad = [int](($w - $h) / 2)
  $g.DrawImage($srcBmp, (New-Object System.Drawing.Rectangle(0, 0, $w, $pad)), (New-Object System.Drawing.Rectangle(0, 0, $w, 1)), [System.Drawing.GraphicsUnit]::Pixel)
  $g.DrawImage($srcBmp, (New-Object System.Drawing.Rectangle(0, ($side - $pad), $w, $pad)), (New-Object System.Drawing.Rectangle(0, ($h - 1), $w, 1)), [System.Drawing.GraphicsUnit]::Pixel)
  $g.DrawImage($srcBmp, 0, $pad, $w, $h)
}
$g.Dispose()

$out = New-Object System.Drawing.Bitmap(1024, 1024)
$g2 = [System.Drawing.Graphics]::FromImage($out)
$g2.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g2.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
$g2.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
$g2.DrawImage($sq, 0, 0, 1024, 1024)
$g2.Dispose()
$out.Save($outPath, [System.Drawing.Imaging.ImageFormat]::Png)
Write-Output "app-icon.png saved (1024x1024)"
$srcBmp.Dispose(); $srcImg.Dispose(); $sq.Dispose(); $out.Dispose()
