@echo off
rem OriEra Studio one-shot packaging script (Windows).
rem Outputs: src-tauri\target\release\bundle\nsis\*.exe (NSIS installer)
rem          src-tauri\target\release\oriera-studio.exe (bare exe; with the
rem          engine-snapshot directory next to it = the portable build)
rem
rem ---- Toolchain routing (P2 cleanup; W6A field notes, 2026-11) --------
rem Two routes:
rem   1) msvc-local (preferred): VS Build Tools (C++ workload) + a rustup
rem      *-pc-windows-msvc toolchain. W6A measured: this route builds
rem      tauri end to end (vite build + cargo release + NSIS all green).
rem   2) mingw (fallback): MinGW-w64 (%USERPROFILE%\mingw-tools\mingw64\bin)
rem      + a rustup *-pc-windows-gnu toolchain. W6A measured: mingw fails
rem      to link the tauri dependency tree. The old script hard-wired this
rem      route (with a PowerShell-style $env:USERPROFILE prefix that never
rem      expands in cmd); it is demoted to fallback here.
rem Routing order (deterministic):
rem   a) probe rust-toolchain.toml / rustup directory override: an
rem      explicitly pinned GNU toolchain is respected -> mingw;
rem   b) otherwise default to msvc-local when present ("exists, use it"):
rem      VS C++ toolset detected AND a rustup msvc toolchain installed ->
rem      select per-process via RUSTUP_TOOLCHAIN (machine-level
rem      `rustup default` is never touched);
rem   c) otherwise fall back to mingw (best effort; a missing gcc warns
rem      but does not abort).
rem
rem NOTE: keep this file ASCII-only. cmd decodes batch files with the
rem console codepage (CP936 on Chinese Windows); UTF-8 Chinese comments
rem get GBK-paired across bytes and can swallow a line's CRLF or its
rem `rem` prefix, turning comment lines into commands (reproduced with
rem the old Chinese header: "'vite' is not recognized as a command").

setlocal EnableExtensions
cd /d "%~dp0.."

set "MINGW_BIN=%USERPROFILE%\mingw-tools\mingw64\bin"
set "ROUTE="
set "ACTIVE_TC="
set "MSVC_TC="
set "MSVC_FOUND="

rem -- a) rust-toolchain.toml / rustup override explicitly pins gnu? ------
if exist rust-toolchain.toml (
  for /f "usebackq tokens=2 delims== " %%a in (`findstr /b /i /c:"channel" rust-toolchain.toml`) do call :pick_triplet %%a
)
if not defined ROUTE (
  for /f "delims=" %%t in ('rustup show active-toolchain 2^>nul') do call :pick_pinned_gnu "%%t"
)

rem -- b) default msvc-local (exists, use it) ------------------------------
if not defined ROUTE call :probe_msvc_local

rem -- c) mingw fallback ---------------------------------------------------
if not defined ROUTE set "ROUTE=mingw"

rem note: echo text inside this block must not contain unescaped parens -
rem a ")" would close the block early, so route messages go through a var
set "ROUTE_MSG="
if /i "%ROUTE%"=="msvc" (
  set "ROUTE_MSG=[0/3] toolchain route: msvc-local - W6A: builds tauri - VS: %MSVC_FOUND%"
  set "RUSTUP_TOOLCHAIN=%MSVC_TC%"
) else (
  set "ROUTE_MSG=[0/3] toolchain route: mingw fallback - W6A: used when msvc unavailable"
  if exist "%MINGW_BIN%\gcc.exe" (
    set "PATH=%MINGW_BIN%;%PATH%"
  ) else (
    echo [warn] %MINGW_BIN%\gcc.exe not found - cargo may fail at link time
  )
)
echo %ROUTE_MSG%
if defined RUSTUP_TOOLCHAIN (
  echo       RUSTUP_TOOLCHAIN=%RUSTUP_TOOLCHAIN%
) else if defined ACTIVE_TC (
  echo       active rustup toolchain: %ACTIVE_TC%
)

echo [1/3] Installing frontend dependencies...
call npm install || goto :err

echo [2/3] Building installer (vite build + cargo release + NSIS)...
call npx tauri build || goto :err

echo [3/3] Packing portable zip...
powershell -NoProfile -Command ^
  "$d='portable\OriEraStudio'; if(Test-Path $d){Remove-Item -Recurse -Force $d}; New-Item -ItemType Directory -Force $d | Out-Null;" ^
  "Copy-Item 'src-tauri\target\release\oriera-studio.exe' $d; Copy-Item 'src-tauri\target\release\WebView2Loader.dll' $d; Copy-Item 'engine-snapshot' \"$d\engine-snapshot\" -Recurse;" ^
  "Compress-Archive -Path $d -DestinationPath 'portable\OriEraStudio-portable.zip' -Force" || goto :err

echo.
echo Done.
echo   installer: src-tauri\target\release\bundle\nsis\
echo   portable:  portable\OriEraStudio-portable.zip
pause
exit /b 0

:err
echo Build FAILED - see output above.
pause
exit /b 1

rem -- subroutines ----------------------------------------------------------

rem :pick_triplet <toolchain triplet/channel> - a "-gnu" suffix means mingw
:pick_triplet
set "TC=%~1"
if not defined TC goto :eof
if /i "%TC:~-4%"=="-gnu" set "ROUTE=mingw"
goto :eof

rem :pick_pinned_gnu <rustup show active-toolchain line> - only an
rem explicitly pinned gnu toolchain (a line NOT ending in "(default)")
rem counts as a pin and routes to mingw. A machine-level gnu default does
rem not count: it falls through to the msvc-local default probe.
:pick_pinned_gnu
set "ACTIVE_TC=%~1"
if not defined ACTIVE_TC goto :eof
echo %ACTIVE_TC%| findstr /i "gnu" >nul || goto :eof
echo %ACTIVE_TC%| findstr /c:"(default)" >nul && goto :eof
set "ROUTE=mingw"
goto :eof

rem :probe_msvc_local - msvc route requires BOTH a VS C++ toolset and an
rem installed rustup msvc toolchain ("exists, use it")
:probe_msvc_local
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if exist "%VSWHERE%" (
  for /f "usebackq delims=" %%p in (`"%VSWHERE%" -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2^>nul`) do set "MSVC_FOUND=%%p"
)
if not defined MSVC_FOUND (
  where cl >nul 2>nul && set "MSVC_FOUND=cl.exe on PATH"
)
if not defined MSVC_FOUND goto :eof
rem prefer a stable msvc toolchain, then any msvc one
for /f "tokens=1" %%t in ('rustup toolchain list 2^>nul ^| findstr /i "msvc" ^| findstr /b /i "stable-"') do set "MSVC_TC=%%t"
if not defined MSVC_TC (
  for /f "tokens=1" %%t in ('rustup toolchain list 2^>nul ^| findstr /i "msvc"') do set "MSVC_TC=%%t"
)
if not defined MSVC_TC (
  echo [warn] VS detected but rustup has no msvc toolchain; falling back to mingw.
  echo [hint] rustup toolchain install stable-x86_64-pc-windows-msvc enables msvc-local.
  goto :eof
)
set "ROUTE=msvc"
goto :eof
