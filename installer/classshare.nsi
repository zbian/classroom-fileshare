; 课堂共享 ClassShare —— 单一 EXE 安装包（NSIS，原生界面，无脚本）
;
; 产物：ClassShare-Setup.exe
; 安装：%ProgramData%\ClassShare\bin（无空格路径，避免 sc/reg 引号问题）
; 配置：%ProgramData%\ClassShare\svc.json（由 classshare-config.exe 以 UTF-8 写入）
; 静默：ClassShare-Setup.exe /S /ROLE=teacher /ROOM=XK-204 [/PIN=1234]
;       ClassShare-Setup.exe /S /ROLE=student [/NICK=alice] [/ADDR=192.168.1.7:47821] [/ROOM=XK-204]

Unicode true
!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

!define PRODUCT_NAME "课堂共享 ClassShare"
!define PRODUCT_VERSION "0.2.0-rc.11"
!define PRODUCT_PUBLISHER "ClassShare"
!define SERVICE_NAME "ClassShareSvc"

Name "${PRODUCT_NAME}"
OutFile "ClassShare-Setup.exe"
InstallDir "$WINDIR\..\ProgramData\ClassShare\bin"
InstallDirRegKey HKLM "Software\ClassShare" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
XPStyle on
ShowInstDetails show
ShowUninstDetails show

VIProductVersion "0.2.0.11"
VIAddVersionKey "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey "ProductVersion" "${PRODUCT_VERSION}"
VIAddVersionKey "Publisher" "${PRODUCT_PUBLISHER}"
VIAddVersionKey "FileDescription" "${PRODUCT_NAME} 安装程序"
VIAddVersionKey "FileVersion" "${PRODUCT_VERSION}"

!define MUI_ICON "app.ico"
!define MUI_UNICON "app.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\classroom-gui.exe"
!define MUI_FINISHPAGE_RUN_PARAMETERS "--role=$Role"
!define MUI_FINISHPAGE_RUN_TEXT "立即启动课堂共享"

Var ProgramData
Var RoomSet
Var Role
Var Room
Var Pin
Var Nick
Var Addr
Var hRoleTeacher
Var hRoleStudent
Var hRoom
Var hPin
Var hNick
Var hAddr

!insertmacro MUI_PAGE_WELCOME
Page custom RolePageShow RolePageLeave
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "SimpChinese"

Function .onInit
  ReadEnvStr $ProgramData "PROGRAMDATA"
  StrCpy $INSTDIR "$ProgramData\ClassShare\bin"
  StrCpy $Role "teacher"
  StrCpy $Room "XK-204"
  StrCpy $RoomSet 0
  StrCpy $Pin ""
  StrCpy $Nick ""
  StrCpy $Addr ""

  ${GetParameters} $R0
  ${GetOptions} $R0 "/ROLE=" $R1
  ${IfNot} ${Errors}
    StrCpy $Role $R1
  ${EndIf}
  ${GetOptions} $R0 "/ROOM=" $R1
  ${IfNot} ${Errors}
    StrCpy $Room $R1
    StrCpy $RoomSet 1
  ${EndIf}
  ${GetOptions} $R0 "/PIN=" $R1
  ${IfNot} ${Errors}
    StrCpy $Pin $R1
  ${EndIf}
  ${GetOptions} $R0 "/NICK=" $R1
  ${IfNot} ${Errors}
    StrCpy $Nick $R1
  ${EndIf}
  ${GetOptions} $R0 "/ADDR=" $R1
  ${IfNot} ${Errors}
    StrCpy $Addr $R1
  ${EndIf}
  ; 学生端缺省不限定房间（自动发现首个教室）
  ${If} $Role == "student"
  ${AndIf} $RoomSet == 0
    StrCpy $Room ""
  ${EndIf}
FunctionEnd

Function RolePageShow
  !insertmacro MUI_HEADER_TEXT "选择角色与教室信息" "教师端建课广播；学生端自动发现/入班。所有输入均可留空使用默认值。"

  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}

  ${NSD_CreateLabel} 0 0 100% 12u "请选择本机角色："
  Pop $0
  ${NSD_CreateRadioButton} 0 14u 45% 12u "教师端（建课、广播、接收提交）"
  Pop $hRoleTeacher
  ${NSD_CreateRadioButton} 50% 14u 45% 12u "学生端（自动发现、接收、提交）"
  Pop $hRoleStudent
  ${If} $Role == "student"
    ${NSD_SetState} $hRoleStudent 1
  ${Else}
    ${NSD_SetState} $hRoleTeacher 1
  ${EndIf}

  ${NSD_CreateGroupBox} 0 32u 100% 118u "参数（按角色填写；留空用默认）"
  Pop $0

  ${NSD_CreateLabel} 6u 48u 30% 12u "房间码（教师端必填，默认 XK-204）"
  Pop $0
  ${NSD_CreateText} 40u 46u 55% 14u "$Room"
  Pop $hRoom

  ${NSD_CreateLabel} 6u 68u 30% 12u "房间 PIN（可选）"
  Pop $0
  ${NSD_CreateText} 40u 66u 55% 14u "$Pin"
  Pop $hPin

  ${NSD_CreateLabel} 6u 88u 30% 12u "学生昵称（学生端可选）"
  Pop $0
  ${NSD_CreateText} 40u 86u 55% 14u "$Nick"
  Pop $hNick

  ${NSD_CreateLabel} 6u 108u 30% 12u "教师机地址（学生端可选，如 192.168.1.7:47821）"
  Pop $0
  ${NSD_CreateText} 40u 106u 55% 14u "$Addr"
  Pop $hAddr

  ${NSD_CreateLabel} 6u 128u 100% 16u "提示：同一局域网学生端可留空地址（自动发现）；跨网段必须填写教师机地址。"
  Pop $0

  nsDialogs::Show
FunctionEnd

Function RolePageLeave
  ${NSD_GetState} $hRoleStudent $0
  ${If} $0 == ${BST_CHECKED}
    StrCpy $Role "student"
  ${Else}
    StrCpy $Role "teacher"
  ${EndIf}
  ${NSD_GetText} $hRoom $Room
  ${NSD_GetText} $hPin $Pin
  ${NSD_GetText} $hNick $Nick
  ${NSD_GetText} $hAddr $Addr

  ${If} $Role == "teacher"
  ${AndIf} $Room == ""
    StrCpy $Room "XK-204"
  ${EndIf}
FunctionEnd

Section "Install"
  SetRegView 64
  SetOutPath "$INSTDIR"
  SetOverwrite on
  File "bin\classroom-svc.exe"
  File "bin\classroom-gui.exe"
  File "bin\classshare-config.exe"

  ; 1) 幂等：停掉并删除旧服务
  DetailPrint "停止/删除旧服务（若有）..."
  nsExec::ExecToLog '"$SYSDIR\sc.exe" stop ${SERVICE_NAME}'
  Sleep 1500
  nsExec::ExecToLog '"$SYSDIR\sc.exe" delete ${SERVICE_NAME}'
  Sleep 1000

  ; 2) 写 svc.json / gui.json（UTF-8，支持中文昵称）
  DetailPrint "写入配置（mode=$Role room=$Room）..."
  nsExec::ExecToLog '"$INSTDIR\classshare-config.exe" --dir "$ProgramData\ClassShare" --mode "$Role" --room "$Room" --pin "$Pin" --nick "$Nick" --addr "$Addr" --gui-file "$APPDATA\ClassShare\gui.json"'
  Pop $0
  ${If} $0 != "0"
    MessageBox MB_ICONSTOP "写入配置失败（退出码 $0），安装中止。"
    Abort
  ${EndIf}

  ; 3) 注册服务（Session 0、开机自启、失败自动重启）
  DetailPrint "注册服务 ${SERVICE_NAME}..."
  nsExec::ExecToLog '"$SYSDIR\sc.exe" create ${SERVICE_NAME} binPath= "$INSTDIR\classroom-svc.exe" start= auto'
  nsExec::ExecToLog '"$SYSDIR\sc.exe" description ${SERVICE_NAME} "课堂共享后台服务（ClassShare）"'
  nsExec::ExecToLog '"$SYSDIR\sc.exe" failure ${SERVICE_NAME} reset= 86400 actions= restart/5000/restart/5000/restart/30000'

  ; 4) 防火墙：UDP 47820（两种角色都需要）；教师端另加 TCP 47821
  DetailPrint "配置防火墙规则..."
  nsExec::ExecToLog '"$SYSDIR\netsh.exe" advfirewall firewall delete rule name="ClassShare Discovery UDP"'
  nsExec::ExecToLog '"$SYSDIR\netsh.exe" advfirewall firewall add rule name="ClassShare Discovery UDP" dir=in action=allow protocol=UDP localport=47820 program="$INSTDIR\classroom-svc.exe"'
  ${If} $Role == "teacher"
    nsExec::ExecToLog '"$SYSDIR\netsh.exe" advfirewall firewall delete rule name="ClassShare Service TCP"'
    nsExec::ExecToLog '"$SYSDIR\netsh.exe" advfirewall firewall add rule name="ClassShare Service TCP" dir=in action=allow protocol=TCP localport=47821 program="$INSTDIR\classroom-svc.exe"'
  ${Else}
    nsExec::ExecToLog '"$SYSDIR\netsh.exe" advfirewall firewall delete rule name="ClassShare Service TCP"'
  ${EndIf}

  ; 5) 启动服务
  DetailPrint "启动服务..."
  nsExec::ExecToLog '"$SYSDIR\sc.exe" start ${SERVICE_NAME}'

  ; 6) GUI 登录自启（带 --role，保证手动启动角色也正确）
  ; 评审 M6：UAC 输管理员凭据时 HKCU 会落到管理员 hive，改用 HKLM（全机生效）
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Run" "ClassShareGUI" "$\"$INSTDIR\classroom-gui.exe$\" --role=$Role"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ClassShareGUI"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ClassShareSvc"

  ; 6b) 资源管理器右键「课堂共享」（HKLM，全机生效；verb 名与 scripts/register-context-menu.ps1 一致）
  ; 评审 M5：MultiSelectModel=Player 让多选一次调用并传全部路径（%* 裸露展开）
  WriteRegStr HKLM "Software\Classes\*\shell\课堂共享" "" "课堂共享"
  WriteRegStr HKLM "Software\Classes\*\shell\课堂共享" "Icon" "$INSTDIR\classroom-gui.exe"
  WriteRegStr HKLM "Software\Classes\*\shell\课堂共享" "MultiSelectModel" "Player"
  WriteRegStr HKLM "Software\Classes\*\shell\课堂共享\command" "" "$\"$INSTDIR\classroom-gui.exe$\" %*"
  ; 清理旧版本写入 HKCU 的菜单（rc.9 及更早）
  DeleteRegKey HKCU "Software\Classes\*\shell\课堂共享"

  ; 7) 卸载信息 + 卸载器
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "Software\ClassShare" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ClassShare" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ClassShare" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ClassShare" "Publisher" "${PRODUCT_PUBLISHER}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ClassShare" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ClassShare" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ClassShare" "DisplayIcon" "$\"$INSTDIR\classroom-gui.exe$\""
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ClassShare" "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ClassShare" "NoRepair" 1

  DetailPrint "安装完成：角色=$Role 房间=$Room 目录=$INSTDIR"
SectionEnd

Function un.onInit
  IfSilent un_silent_ok
  MessageBox MB_YESNO|MB_ICONQUESTION "确定卸载课堂共享？$\r$\n$\r$\n共享数据（%ProgramData%\ClassShare\data）会保留，如需删除请手动清理。" IDYES un_silent_ok
  Abort
  un_silent_ok:
FunctionEnd

Section "Uninstall"
  SetRegView 64
  nsExec::ExecToLog '"$SYSDIR\sc.exe" stop ${SERVICE_NAME}'
  Sleep 1500
  nsExec::ExecToLog '"$SYSDIR\sc.exe" delete ${SERVICE_NAME}'
  Sleep 800

  nsExec::ExecToLog '"$SYSDIR\netsh.exe" advfirewall firewall delete rule name="ClassShare Discovery UDP"'
  nsExec::ExecToLog '"$SYSDIR\netsh.exe" advfirewall firewall delete rule name="ClassShare Service TCP"'

  DeleteRegValue HKLM "Software\Microsoft\Windows\CurrentVersion\Run" "ClassShareGUI"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ClassShareGUI"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ClassShareSvc"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ClassShare"
  DeleteRegKey HKLM "Software\ClassShare"
  DeleteRegKey HKLM "Software\Classes\*\shell\课堂共享"
  DeleteRegKey HKCU "Software\Classes\*\shell\课堂共享"

  Delete "$INSTDIR\classroom-svc.exe"
  Delete "$INSTDIR\classroom-gui.exe"
  Delete "$INSTDIR\classshare-config.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  RMDir "$ProgramData\ClassShare\bin"
  Delete "$ProgramData\ClassShare\svc.json"
  Delete "$APPDATA\ClassShare\gui.json"
SectionEnd
