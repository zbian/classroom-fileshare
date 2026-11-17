# CDP 取证工具（qa 用）

用途：GitHub Actions 停用后，GUI UI 判据改为「dev-gui E2E(CDP) 出证据 + qa 复核」；
本脚本让 QA 也能**独立复跑**同一取证（Lead 2026-11-18 转发的 dev-gui CDP 方法）。

## 关键坑（dev-gui 实测，务必遵守）

1. **端口必须在 WinNAT 排除段之外**。本机 `9222/9223/1420` 被排除 → 绑定 EACCES、`/json` 永不监听。
   自检：`netsh int ipv4 show excludedportrange protocol=tcp`，或试绑 19223（本机可用）。
2. **启动前先杀旧 GUI/WebView2**，否则复用旧浏览器进程、新端口不监听：
   `Get-Process classroom-gui,msedgewebview2 -ErrorAction SilentlyContinue | Stop-Process -Force`
3. **环境变量必须设在启动进程的父 shell**（`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`）。
4. release 构建亦可（内嵌前端，无需 vite）；debug 构建需同时跑 `pnpm dev`（devUrl 1517）。
5. 就绪判据：约 5–9 s 后 `http://127.0.0.1:<port>/json` 返回 JSON 数组，含 `title=课堂共享` 与 `webSocketDebuggerUrl`。
6. **主窗默认隐藏**：先 `window.__TAURI__.core.invoke('open_main')` 再截图/断言。
7. **截图前双 rAF**：`new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(()=>r(1))))`，否则可能截到未绘制帧。

## 启动（示例：teacher，19223）

```powershell
Get-Process classroom-gui,msedgewebview2 -ErrorAction SilentlyContinue | Stop-Process -Force
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=19223'
$env:LOCALAPPDATA = '<iso>\teacher'; $env:APPDATA = '<iso>\teacher\appdata'; $env:PROGRAMDATA = '<iso>\teacher\pd'
Start-Process .\target\release\classroom-gui.exe -ArgumentList '--role=teacher'
# 就绪
Invoke-WebRequest http://127.0.0.1:19223/json -UseBasicParsing | Select-Object -ExpandProperty Content
```

## 用 PowerShell ClientWebSocket 发 CDP 命令（无需 pip 安装）

```powershell
function Invoke-Cdp([string]$wsUrl, [string]$method, [hashtable]$params, [int]$id = 1) {
  $ws = New-Object System.Net.WebSockets.ClientWebSocket
  $ws.ConnectAsync([Uri]$wsUrl, [Threading.CancellationToken]::None).Wait()
  $msg = @{ id = $id; method = $method; params = $params } | ConvertTo-Json -Depth 12 -Compress
  $bytes = [Text.Encoding]::UTF8.GetBytes($msg)
  $ws.SendAsync((New-Object ArraySegment[byte] -ArgumentList @(,$bytes)),
      [System.Net.WebSockets.WebSocketMessageType]::Text, $true,
      [Threading.CancellationToken]::None).Wait()
  $buf = New-Object byte[] 65536; $sb = New-Object Text.StringBuilder
  do {
    $seg = New-Object ArraySegment[byte] -ArgumentList @(,$buf)
    $res = $ws.ReceiveAsync($seg, [Threading.CancellationToken]::None).Result
    [void]$sb.Append([Text.Encoding]::UTF8.GetString($buf, 0, $res.Count))
  } while (-not $res.EndOfMessage)
  $ws.Dispose()
  return $sb.ToString()
}

# 取目标
$targets = (Invoke-WebRequest http://127.0.0.1:19223/json -UseBasicParsing).Content | ConvertFrom-Json
$main = $targets | Where-Object { $_.url -match 'main\.html' } | Select-Object -First 1
# 打开主窗（在 icon 页执行）
$icon = $targets | Where-Object { $_.url -match 'icon\.html' } | Select-Object -First 1
Invoke-Cdp $icon.webSocketDebuggerUrl 'Runtime.evaluate' @{
  expression = "window.__TAURI__.core.invoke('open_main')"; awaitPromise = $true; returnByValue = $true }
# 断言/取数（主窗）
Invoke-Cdp $main.webSocketDebuggerUrl 'Runtime.evaluate' @{
  expression = "document.body.innerText"; returnByValue = $true }
# 截图（双 rAF 后）
Invoke-Cdp $main.webSocketDebuggerUrl 'Runtime.evaluate' @{
  expression = "new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(()=>r(1))))"; awaitPromise = $true }
Invoke-Cdp $main.webSocketDebuggerUrl 'Page.captureScreenshot' @{ format = 'png' }
```

返回体里 `result.result.value`（`Runtime.evaluate`）或 `result.data`（base64 PNG，`Page.captureScreenshot`）。

## QA 复核清单（dev-gui 证据到手时逐条核）

| # | 检查 | 方法 |
| --- | --- | --- |
| 1 | 命令可复现 | 按其记录的命令本机跑一次，能连上 CDP 并取到同一断言 |
| 2 | 截图内容与判据一致 | 截图需可见关键文本（Tab 名、分组计数、确认框逐字文案、toast 台数） |
| 3 | DOM 断言真实 | 断言表达式应取运行期 DOM（非静态桩）；必要时抽查一处故意改数据看断言是否随之变化 |
| 4 | 环境说明 | 记录 GUI 构建（`build-gui.ps1` / debug）、数据根隔离、online_count 来源 |

## 协作约定

- **GUI 进程只由一方启动**：dev-gui 做 E2E 时 QA 不启动；QA 需要窗口时先与 dev-gui 约定时间（启动前告知），且使用独立数据根与独立 CDP 端口（如 19223），跑完只杀自己路径的进程。
- 桌面若有 `Windows 安全中心`(`Shell_SystemDim`/`PickerHost`) 模态遮罩，鼠标点击会被吞；先清理再自动化（QA 已记录一次，见 `evidence/M2/gui-blocker.txt`）。
