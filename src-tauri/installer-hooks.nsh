; OriEra Studio NSIS 安装钩子（tauri bundle.windows.nsis.installerHooks 挂载）
; 装后检测火绒安全：其浏览器防护会拦截内嵌游戏窗口（白屏），提醒用户加信任区

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToStack 'cmd /c tasklist /NH | find /I "usysdiag.exe"'
  Pop $0   ; 输出
  Pop $1   ; 退出码（0 = find 找到了 usysdiag 进程）
  StrCmp $1 "0" huorong_found huorong_done
huorong_found:
  MessageBox MB_OK|MB_ICONINFORMATION "检测到火绒安全软件正在运行。$\n$\n火绒的浏览器防护可能拦截 OriEra 的游戏窗口（表现为窗口白屏）。建议任选其一：$\n$\n  1. 打开火绒 → 信任区 → 添加目录 → 选择本程序安装目录$\n  2. 游玩时点击项目卡片上的「🌐 浏览器」按钮（不受影响）$\n$\n感谢理解 —— OriEra Studio"
huorong_done:
!macroend
