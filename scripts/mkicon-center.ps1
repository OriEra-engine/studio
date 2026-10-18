Add-Type -AssemblyName System.Drawing
$dir = "C:\Users\Lenovo\.zcode\workspace\default\oriera-studio"
$srcImg = [System.Drawing.Image]::FromFile("$dir\icon-source.jpg")
$w = $srcImg.Width; $h = $srcImg.Height
$srcBmp = New-Object System.Drawing.Bitmap($srcImg)

# emblem bbox measured at threshold 140: L=82 T=406 R=884 B=1172; pad to catch soft edges
$pad = 12
$L = [Math]::Max(0, 82 - $pad); $T = [Math]::Max(0, 406 - $pad)
$R = [Math]::Min($w - 1, 884 + $pad); $B = [Math]::Min($h - 1, 1172 + $pad)
$ew = $R - $L + 1; $eh = $B - $T + 1
Write-Output ("emblem region: " + $ew + "x" + $eh + " at (" + $L + "," + $T + ")")

# background color = average of the four corners
function CornerAvg($b) {
  $pts = @([System.Drawing.Point]::new(4, 4), [System.Drawing.Point]::new($b.Width - 5, 4), [System.Drawing.Point]::new(4, $b.Height - 5), [System.Drawing.Point]::new($b.Width - 5, $b.Height - 5))
  $r = 0; $g = 0; $bl = 0
  foreach ($p in $pts) { $c = $b.GetPixel($p.X, $p.Y); $r += $c.R; $g += $c.G; $bl += $c.B }
  return [System.Drawing.Color]::FromArgb(255, [int]($r / 4), [int]($g / 4), [int]($bl / 4))
}
$bg = CornerAvg $srcBmp
Write-Output ("bg color: " + $bg.R + "," + $bg.G + "," + $bg.B)

# extract emblem
$emblem = New-Object System.Drawing.Bitmap($ew, $eh)
$ge = [System.Drawing.Graphics]::FromImage($emblem)
$ge.DrawImage($srcBmp, (New-Object System.Drawing.Rectangle(0, 0, $ew, $eh)), (New-Object System.Drawing.Rectangle($L, $T, $ew, $eh)), [System.Drawing.GraphicsUnit]::Pixel)
$ge.Dispose()

# scale emblem so max side = 82% of 1024, paste dead-center on bg canvas
$target = [int](1024 * 0.82)
$scale = $target / [double]([Math]::Max($ew, $eh))
$nw = [int]($ew * $scale); $nh = [int]($eh * $scale)
Write-Output ("scaled emblem: " + $nw + "x" + $nh + " -> fill " + [int]($nw / 1024.0 * 100) + "% x " + [int]($nh / 1024.0 * 100) + "%")

$out = New-Object System.Drawing.Bitmap(1024, 1024)
$g = [System.Drawing.Graphics]::FromImage($out)
$g.Clear($bg)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
$g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
$x0 = [int]((1024 - $nw) / 2); $y0 = [int]((1024 - $nh) / 2)
$g.DrawImage($emblem, $x0, $y0, $nw, $nh)
$g.Dispose()
$out.Save("$dir\app-icon.png", [System.Drawing.Imaging.ImageFormat]::Png)
Write-Output ("pasted at (" + $x0 + "," + $y0 + "), margins L/R=" + $x0 + "/" + (1024 - $x0 - $nw) + " T/B=" + $y0 + "/" + (1024 - $y0 - $nh))
Write-Output "app-icon.png saved"
$srcBmp.Dispose(); $srcImg.Dispose(); $emblem.Dispose(); $out.Dispose()
