# #16B 真实链路 E2E 复现记录（学员发送 → 教师「学生提交」树）

> 结论：**真实链路（非 mock）已跑通**，可作为 M2 TC-04/05 的 UI 判据取证来源。
> 证据：`01-student-send-dialog.png`、`02-student-send-toast.png`、`03-teacher-tree-real-submission.png`
> 另有服务层日志/真实 IPC 断言（见下）。

## 关键前提（GUI 真实数据模式）

dev 构建默认使用 **MockGuiClient**（`getClient()`：`import.meta.env.DEV && !location.hash.includes('realIpc')`）。
做真实链路取证必须让 GUI 走 **RealGuiClient**：打开主窗后执行

```js
location.hash = 'realIpc'; location.reload()
```

（本记录中的步骤都做了这一步；截图为 `#realIpc` 模式下的真实数据。）

## 环境（隔离，互不污染；无需管理员）

```powershell
$w = "<repo>\.wt\feat-16b-send"
$T = Join-Path $env:TEMP "e16d"; $A = "$T\tea"; $B = "$T\stu"; $PD = "$T\pd"
New-Item -ItemType Directory -Force -Path "$A\ClassShare","$B\ClassShare",$PD | Out-Null
# 教师配置
'{ "mode":"teacher","room_code":"XK-204","tcp_port":47821 }' | Set-Content "$A\ClassShare\svc.json" -Encoding utf8NoBOM
# 学员配置（manual_addr 指向本机教师；昵称 e16stu）
'{ "mode":"student","room_code":"XK-204","manual_addr":"127.0.0.1:47821","nickname":"e16stu" }' | Set-Content "$B\ClassShare\svc.json" -Encoding utf8NoBOM
# GUI 角色 + 跳过昵称弹窗
'{ "version":1,"role":"student","icon":{"pausedTips":false,"nicknameDone":true} }' | Set-Content "$B\ClassShare\gui.json" -Encoding utf8NoBOM
'{ "version":1,"role":"teacher","icon":{"pausedTips":false,"nicknameDone":true} }' | Set-Content "$A\ClassShare\gui.json" -Encoding utf8NoBOM
```

## 启动（顺序敏感）

```powershell
# 1) vite（debug GUI 的 devUrl 端口 1517）
cd "$w\apps\classroom-gui\ui"; pnpm dev        # 单独窗口保持运行

# 2) 教师 svc → 学员 svc（各自独立 LOCALAPPDATA；PROGRAMDATA 指向空目录避免读到本机真实 token）
$svc = "$w\target\debug\classroom-svc.exe"
$sh = New-Object -ComObject WScript.Shell
[void]$sh.Run("cmd /c set LOCALAPPDATA=$A&& set PROGRAMDATA=$PD&& `"$svc`" --console 2>`"$T\tea.log`"",0,$false)
Start-Sleep 3
[void]$sh.Run("cmd /c set LOCALAPPDATA=$B&& set PROGRAMDATA=$PD&& `"$svc`" --console 2>`"$T\stu.log`"",0,$false)
Start-Sleep 5
# 判据：2 个 svc + 2 条管道；教师日志有「教师端启动 room=XK-204」；学员日志有「已加入房间」

# 3) 学员 GUI（真实数据；CDP 端口 19227）
$exe = "$w\target\debug\classroom-gui.exe"
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=19227'
[void]$sh.Run("cmd /c set LOCALAPPDATA=$B&& set PROGRAMDATA=$PD&& set APPDATA=$B&& set WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=19227&& `"$exe`" --role=student 2>`"$T\gui-stu.log`"",0,$false)
# 判据（gui-stu.log）：[role] 解析为 student（来源 Arg）→ [ipc] 服务管道已连接（role=Student）
```

> 注意：svc `--console` 在 stdin 关闭后会退出，**整条链路需在同一分钟内完成**（脚本连跑即可）。
> 另：single-instance 插件限制同机只能同时运行 1 个 GUI → 学员 GUI 取证完 `Stop-Process classroom-gui` 再起教师 GUI。

## CDP 操作（Python + websocket-client）

```python
# 连接
t = [x for x in json.load(urllib.request.urlopen("http://127.0.0.1:19227/json"))
     if x["url"].startswith("http://localhost:1517/main.html")][0]
ws = websocket.create_connection(t["webSocketDebuggerUrl"])
# 真实数据模式 + 显示主窗
Runtime.evaluate: "location.hash='realIpc'; location.reload()"   # 等 5s 后重连
Runtime.evaluate(awaitPromise): "window.__TAURI__.core.invoke('open_main').then(()=>'ok')"
# 截图（先双 rAF 等合成器提交，否则可能拿到上一帧）
Runtime.evaluate(awaitPromise): "new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(()=>r(1))))"
# 学员：点「+ 粘贴文本」→ 填 .modal input/textarea → 点「共享」→ 0.9s 内读 toast
# 教师：点含「学生提交」的 .app-tab → 读 .sub-node / .grow .name
```

## 实测观察（2026-11-18 00:26 本机）

| 断言 | 结果 |
| --- | --- |
| 学员工具栏 | `+ 添加文件`、`+ 粘贴文本` 均 enabled；`发送对象：仅教师` chip 存在 |
| 发送确认框 | `接收对象：仅教师`（PRD 6.2：无「发送给全班」选项） |
| 发送成功 toast | **已发送给教师** |
| 学员 svc 日志 | 学员加入房间 → （发送时）上行 |
| 教师 svc 日志 | `学员提交已登记，等待内容帧 … owner=e16stu item_id=…` → `学员提交入库 owner=e16stu … title=16B真实链路提交 version=1` |
| 教师真实 IPC `Submissions` | `[{"owner_id":"e16stu","count":1,"latest_at":1794932775}]` |
| 教师真实 IPC `List` | `[{"title":"16B真实链路提交","owner":"e16stu","owner_role":"Student","audience":"Teacher","kind":"Text","sync":"Synced"}]` |
| 教师 GUI「学生提交」树（realIpc） | 左树 `e16stu · 1`（最近时间 00:26）；右表 1 行 `16B真实链路提交`（只读） |

## 已知注意点

- `latest_at` 是 **Unix 秒**（#94 契约），条目 `updated_at` 是毫秒；左树渲染需 ×1000（#16B 已修正，否则显示 1970/01-22）。
- 教师 GUI 与学员 GUI 不能同时运行（single-instance），故分两段取证。
- 本记录不覆盖 TC-08 的「离线 agent 上线补删」时序，由 qa 另验。
