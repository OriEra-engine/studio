$ErrorActionPreference = "Continue"
Add-Type -AssemblyName System.Drawing

# 1. ship the black-logo ico next to the app (stable path, fresh cache key)
$dir = "C:\Users\Lenovo\AppData\Local\OriEra Studio"
$ico = Join-Path $dir "app-icon.ico"
Copy-Item "C:\Users\Lenovo\.zcode\workspace\default\oriera-studio\src-tauri\icons\icon.ico" $ico -Force
Write-Output ("ico shipped: " + (Get-Item $ico).Length + " bytes")

# verify the ico really is the black logo (16x16 entry, first entry in file)
$bytes = [System.IO.File]::ReadAllBytes($ico)
$w = $bytes[6]; $h = $bytes[7]
Write-Output ("first ico entry: ${w}x${h}")

# 2. point both shortcuts at the standalone ico
$sh = New-Object -ComObject WScript.Shell
foreach ($p in @(
  "C:\Users\Lenovo\Desktop\OriEra Studio.lnk",
  "C:\Users\Lenovo\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\OriEra Studio.lnk"
)) {
  if (Test-Path $p) {
    $lnk = $sh.CreateShortcut($p)
    $lnk.TargetPath = (Join-Path $dir "oriera-studio.exe")
    $lnk.WorkingDirectory = $dir
    $lnk.IconLocation = "$ico,0"
    $lnk.Save()
    Write-Output ("lnk updated: $p -> ico")
  }
}

# 3. kill icon cache files with handles released by restarting explorer
ie4uinit.exe -show 2>$null
Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2
$cache = "$env:LOCALAPPDATA\Microsoft\Windows\Explorer"
Get-ChildItem $cache -Filter "iconcache*" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
Get-ChildItem $cache -Filter "thumbcache*" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
if (-not (Get-Process explorer -ErrorAction SilentlyContinue)) { Start-Process explorer }
Start-Sleep -Seconds 3
ie4uinit.exe -show 2>$null
Write-Output "explorer restarted, icon cache rebuilt"
Write-Output ("explorer running: " + [bool](Get-Process explorer -ErrorAction SilentlyContinue))
