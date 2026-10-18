$ErrorActionPreference = "Continue"
$dir = "C:\Users\Lenovo\AppData\Local\OriEra Studio"
$setup = "C:\Users\Lenovo\.zcode\workspace\default\oriera-studio\src-tauri\target\release\bundle\nsis\OriEra Studio_0.3.0_x64-setup.exe"

Stop-Process -Name oriera-studio -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

$un = Join-Path $dir "uninstall.exe"
if (Test-Path $un) {
  $p = Start-Process -FilePath $un -ArgumentList "/S" -PassThru -Wait
  for ($i = 0; $i -lt 24; $i++) { if (-not (Test-Path $dir)) { break }; Start-Sleep -Milliseconds 500 }
  Write-Output ("uninstalled, dir removed: " + (-not (Test-Path $dir)))
}

$p2 = Start-Process -FilePath $setup -ArgumentList "/S", "/D=$dir" -PassThru -Wait
Write-Output ("install exit: " + $p2.ExitCode)
Start-Sleep -Seconds 2
$exe = Join-Path $dir "oriera-studio.exe"
$i = Get-Item $exe
Write-Output ("exe: " + $i.Length + " bytes mtime=" + $i.LastWriteTime)
$rp = Get-ItemProperty "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\OriEra Studio"
Write-Output ("registry: " + $rp.DisplayVersion)

# rewrite desktop shortcut to force icon re-resolution
$sh = New-Object -ComObject WScript.Shell
$lnk = $sh.CreateShortcut("C:\Users\Lenovo\Desktop\OriEra Studio.lnk")
$lnk.TargetPath = $exe
$lnk.WorkingDirectory = $dir
$lnk.IconLocation = "$exe,0"
$lnk.Save()

# icon cache refresh
ie4uinit.exe -show 2>$null
$iconCacheDir = "$env:LOCALAPPDATA\Microsoft\Windows\Explorer"
Get-ChildItem $iconCacheDir -Filter "iconcache*" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
ie4uinit.exe -show 2>$null
Write-Output "shortcut rewritten + icon cache refreshed"

# smoke
Start-Process $exe
Start-Sleep -Seconds 7
$p3 = Get-Process oriera-studio -ErrorAction SilentlyContinue
if ($p3) {
  $port = (Get-NetTCPConnection -OwningProcess $p3.Id -State Listen -ErrorAction SilentlyContinue | Where-Object LocalAddress -eq "127.0.0.1" | Select-Object -First 1).LocalPort
  Write-Output ("health: " + (Invoke-RestMethod "http://127.0.0.1:$port/health" | ConvertTo-Json -Compress))
  Stop-Process -Id $p3.Id -Force
  Write-Output "smoke ok"
} else { Write-Output "APP NOT RUNNING" }

# sync portable + desktop installer
$root = "C:\Users\Lenovo\.zcode\workspace\default\oriera-studio"
$d = Join-Path $root "portable\OriEraStudio"
if (Test-Path $d) { Remove-Item -Recurse -Force $d }
New-Item -ItemType Directory -Force $d | Out-Null
Copy-Item (Join-Path $root "src-tauri\target\release\oriera-studio.exe") $d
Copy-Item (Join-Path $root "src-tauri\target\release\WebView2Loader.dll") $d
Copy-Item (Join-Path $root "engine-snapshot") (Join-Path $d "engine-snapshot") -Recurse
$zip = Join-Path $root "portable\OriEraStudio-portable.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $d -DestinationPath $zip
Write-Output ("portable zip: " + [math]::Round((Get-Item $zip).Length / 1MB, 2) + " MB")
Copy-Item $setup "C:\Users\Lenovo\Desktop\OriEra Studio_0.3.0_x64-setup.exe" -Force
Write-Output "desktop installer copy refreshed"
