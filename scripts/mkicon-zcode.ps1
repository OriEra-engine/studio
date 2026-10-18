Add-Type -AssemblyName System.Drawing
$dir = "$PSScriptRoot\.."
$srcImg = [System.Drawing.Image]::FromFile("$dir\icon-source.jpg")
$srcBmp = New-Object System.Drawing.Bitmap($srcImg)

# emblem padded bbox: L=70 T=394 827x791
$L = 70; $T = 394; $ew = 827; $eh = 791

# contrast boost: whites -> pure white, mids brightened, darks crushed (levels-style linear stretch)
$attr = New-Object System.Drawing.Imaging.ImageAttributes
$cm = New-Object System.Drawing.Imaging.ColorMatrix
$cm.Matrix00 = 1.35; $cm.Matrix11 = 1.35; $cm.Matrix22 = 1.35
$cm.Matrix40 = -0.1765; $cm.Matrix41 = -0.1765; $cm.Matrix42 = -0.1765
$attr.SetColorMatrix($cm)

$emblem = New-Object System.Drawing.Bitmap($ew, $eh)
$ge = [System.Drawing.Graphics]::FromImage($emblem)
$dstR = New-Object System.Drawing.Rectangle(0, 0, $ew, $eh)
$srcR = New-Object System.Drawing.Rectangle($L, $T, $ew, $eh)
$ge.DrawImage($srcBmp, $dstR, $srcR.X, $srcR.Y, $srcR.Width, $srcR.Height, [System.Drawing.GraphicsUnit]::Pixel, $attr)
$ge.Dispose()

# canvas: ZCode-style vertical gradient
$out = New-Object System.Drawing.Bitmap(1024, 1024)
$g = [System.Drawing.Graphics]::FromImage($out)
$rect = New-Object System.Drawing.Rectangle(0, 0, 1024, 1024)
$top = [System.Drawing.Color]::FromArgb(255, 22, 24, 26)
$bot = [System.Drawing.Color]::FromArgb(255, 5, 5, 6)
$grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush($rect, $top, $bot, [System.Drawing.Drawing2D.LinearGradientMode]::Vertical)
$g.FillRectangle($grad, $rect)

# soft radial lift behind emblem (subtle focus, ~7% white at center fading out)
$path = New-Object System.Drawing.Drawing2D.GraphicsPath
$path.AddEllipse(-140, -140, 1024 + 280, 1024 + 280)
$pgb = New-Object System.Drawing.Drawing2D.PathGradientBrush($path)
$pgb.CenterColor = [System.Drawing.Color]::FromArgb(20, 255, 255, 255)
$pgb.SurroundColors = @([System.Drawing.Color]::FromArgb(0, 255, 255, 255))
$g.FillPath($pgb, $path)

# emblem at 82%, exact geometric center (on the midline)
$target = [int](1024 * 0.82)
$scale = $target / [double]([Math]::Max($ew, $eh))
$nw = [int]($ew * $scale); $nh = [int]($eh * $scale)
$x0 = [int]((1024 - $nw) / 2)
$y0 = [int]((1024 - $nh) / 2)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
$g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
$g.DrawImage($emblem, $x0, $y0, $nw, $nh)
$g.Dispose()
$out.Save("$dir\app-icon.png", [System.Drawing.Imaging.ImageFormat]::Png)
Write-Output ("emblem " + $nw + "x" + $nh + " at (" + $x0 + "," + $y0 + ") fill=" + [int]($nw / 1024.0 * 100) + "% + contrast boost + soft lift + optical center")
$srcBmp.Dispose(); $srcImg.Dispose(); $emblem.Dispose(); $out.Dispose()
