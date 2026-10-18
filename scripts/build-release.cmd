@echo off
rem OriEra Studio 一键打包脚本（Windows）
rem 前置：node/npm 已装；Rust GNU 工具链 + MinGW-w64（$env:USERPROFILE\mingw-tools\mingw64\bin 在 PATH）
rem 产物：src-tauri\target\release\bundle\nsis\*.exe（安装包）
rem       src-tauri\target\release\oriera-studio.exe（裸 exe，配合 engine-snapshot 目录即为便携版）

set PATH=$env:USERPROFILE\mingw-tools\mingw64\bin;%PATH%
cd /d "%~dp0.."

echo [1/3] 安装前端依赖...
call npm install || goto :err

echo [2/3] 构建安装包（vite build + cargo release + NSIS）...
call npx tauri build || goto :err

echo [3/3] 生成便携版 zip...
powershell -NoProfile -Command ^
  "$d='portable\OriEraStudio'; if(Test-Path $d){Remove-Item -Recurse -Force $d}; New-Item -ItemType Directory -Force $d | Out-Null;" ^
  "Copy-Item 'src-tauri\target\release\oriera-studio.exe' $d; Copy-Item 'src-tauri\target\release\WebView2Loader.dll' $d; Copy-Item 'engine-snapshot' \"$d\engine-snapshot\" -Recurse;" ^
  "Compress-Archive -Path $d -DestinationPath 'portable\OriEraStudio-portable.zip' -Force" || goto :err

echo.
echo 完成！
echo   安装包: src-tauri\target\release\bundle\nsis\
echo   便携版: portable\OriEraStudio-portable.zip
pause
exit /b 0

:err
echo 打包失败，请检查上方输出。
pause
exit /b 1
