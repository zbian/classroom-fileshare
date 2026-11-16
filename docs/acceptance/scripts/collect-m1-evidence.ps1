<#
.SYNOPSIS
  M1 验收证据采集脚本（qa-acceptance / issue #15）。

.DESCRIPTION
  在被测机（教师机 / 学员机）本地运行，一屏输出该机在 M1 验收中需要的取证数据：
  服务与进程状态、仓库与索引概览、E4 约定的日志锚点（broadcast/received/applied）、
  指定条目的落盘文件属性与哈希、会话生命周期日志行。

  日志锚点格式（issue #14 / E4 约定，字段名稳定）：
    <RFC3339 含毫秒> broadcast item_id=<uuid> file=<文件名> size=<字节；文本条目=字符数（E6）>
    <RFC3339 含毫秒> received item_id=<uuid> file=<文件名> size=<字节；文本条目=字符数（E6）>
    <RFC3339 含毫秒> applied  item_id=<uuid> file=<文件名> size=<字节；文本条目=字符数（E6）>
  Delta_node = T(applied, 本机) - T(broadcast, 教师机)；由汇总端合并各机输出计算。

  E4 未合入 main 时的降级口径：改用 -ItemName 取落盘文件 mtime 作为 applied 代理，
  并以会话生命周期行（教师端启动/学员加入/学员离线/已加入/旧连接）为辅证。

  兼容性：Windows PowerShell 5.1 与 PowerShell 7 均可运行（文件须为 UTF-8 BOM）。

.PARAMETER Role
  teacher | student

.PARAMETER DataRoot
  数据根目录。默认按管理员安装取 %PROGRAMDATA%\ClassShare\data；
  降级安装（无管理员）取 %LOCALAPPDATA%\ClassShare\data。

.PARAMETER ItemName
  可选：本次验收关注的文件名（用于输出其落盘属性与哈希、.bak 计数）。

.PARAMETER Since
  可选：仅输出该时间之后（含）的日志行，格式 yyyy-MM-ddTHH:mm:ss。

.PARAMETER OutFile
  可选：将本机输出另存为文件（建议：evidence/M1/<TC>/<机器名>.txt）。

.EXAMPLE
  .\collect-m1-evidence.ps1 -Role student -ItemName "课堂讲义.docx" -OutFile "TC-06-学员A.txt"

.NOTES
  只读脚本：不修改、不删除任何数据；仅读取服务状态、目录、SQLite 索引与日志。
#>
[CmdletBinding()]
param(
    [ValidateSet('teacher', 'student')][string]$Role = 'student',
    [string]$DataRoot = '',
    [string]$ItemName = '',
    [string]$Since = '',
    [string]$OutFile = ''
)

$ErrorActionPreference = 'Continue'
$svcName = 'ClassShareSvc'
$out = New-Object System.Collections.Generic.List[string]

function Emit([string]$line = '') { $script:out.Add($line) }

function Resolve-DataRoot {
    if ($DataRoot) {
        $resolved = Resolve-Path $DataRoot -ErrorAction SilentlyContinue
        if ($resolved) { return $resolved.Path }
        return $DataRoot
    }
    $admin = Join-Path $env:ProgramData 'ClassShare\data'
    if (Test-Path $admin) { return $admin }
    $fallback = Join-Path $env:LOCALAPPDATA 'ClassShare\data'
    if (Test-Path $fallback) { return $fallback }
    return $admin
}

$root = Resolve-DataRoot

Emit ("===== M1 验收取证  role=" + $Role + "  machine=" + $env:COMPUTERNAME + "  at=" + (Get-Date -Format 'yyyy-MM-ddTHH:mm:ss.fff') + " =====")
Emit ("数据根: " + $root)
Emit ("PowerShell: " + $PSVersionTable.PSVersion.ToString())

# 1) 服务与进程
Emit ''
Emit '--- [1] 服务与进程 ---'
$svc = Get-Service -Name $svcName -ErrorAction SilentlyContinue
if ($svc) {
    Emit ("服务 " + $svcName + ": " + $svc.Status)
} else {
    Emit ("服务 " + $svcName + ": 未注册（降级模式或未安装）")
}
foreach ($img in @('classroom-svc', 'classroom-gui')) {
    $p = Get-Process -Name $img -ErrorAction SilentlyContinue
    if ($p) {
        Emit ("进程 " + $img + ".exe: PID " + ($p.Id -join ','))
    } else {
        Emit ("进程 " + $img + ".exe: 未运行")
    }
}
$runKeys = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -ErrorAction SilentlyContinue
$runHit = $runKeys.PSObject.Properties | Where-Object { $_.Name -match 'ClassShare' } | ForEach-Object { $_.Name + '=' + $_.Value }
if ($runHit) { Emit ("HKCU Run 自启项: " + ($runHit -join ' | ')) } else { Emit 'HKCU Run 自启项: 无' }

# 2) 时钟（跨机 Delta 比对的前提）
Emit ''
Emit '--- [2] 时钟（Delta 计算前提） ---'
try {
    # w32tm 输出为本地代码页（中文机 936），经管道会被按控制台编码误解码 → 落临时文件按 ANSI 读回
    $wtFile = Join-Path ([IO.Path]::GetTempPath()) ('cs_w32tm_' + [guid]::NewGuid().ToString('N') + '.txt')
    & cmd /c "w32tm /query /status > `"$wtFile`" 2>&1"
    if (Test-Path $wtFile) {
        foreach ($l in @(Get-Content $wtFile -Encoding Default)) { if ($l.Trim()) { Emit $l } }
        Remove-Item $wtFile -Force -ErrorAction SilentlyContinue
    }
} catch {
    Emit ('w32tm 查询失败: ' + $_)
}
try {
    $tz = Get-CimInstance Win32_TimeZone -ErrorAction SilentlyContinue
    if ($tz) { Emit ('时区: ' + $tz.Caption + '  UTC偏移: ' + $tz.Bias) }
} catch {
    Emit ('时区查询失败: ' + $_)
}
Emit ('本机 Get-Date: ' + (Get-Date -Format 'yyyy-MM-ddTHH:mm:ss.fff'))

# 3) E4 日志锚点
Emit ''
Emit '--- [3] 日志锚点（E4 格式：broadcast/received/applied） ---'
$log = Join-Path $root 'logs\svc.log'
if (Test-Path $log) {
    $logItem = Get-Item $log
    Emit ("日志: " + $log + "  (大小 " + $logItem.Length + " 字节, 最后写入 " + $logItem.LastWriteTime.ToString('yyyy-MM-ddTHH:mm:ss.fff') + ")")
    $lines = @(Get-Content $log -Encoding UTF8)
    if ($Since) { $lines = @($lines | Where-Object { $_ -ge $Since }) }
    # ★折行展开：tracing 的 ANSI writer 会按宽度把一条记录折成多个物理行（续行以空白开头），
    #   实测 `audience="all"` 曾被拆到下一行 → 不展开会截断字段值，故先折叠为逻辑记录。
    $records = New-Object System.Collections.Generic.List[string]
    foreach ($l in $lines) {
        if ($l -match '^\s+\S' -and $records.Count -gt 0) {
            $records[$records.Count - 1] = $records[$records.Count - 1] + ' ' + $l.Trim()
        } else {
            $records.Add($l)
        }
    }
    $anchors = @($records | Select-String -Pattern '(?<![\w])(broadcast|received|applied)(?![\w=])')
    if ($anchors.Count -gt 0) {
        foreach ($a in $anchors) { Emit $a.Line }
    } else {
        Emit '(本机无 broadcast/received/applied 锚点 —— E4 未合入，或本机未参与该次广播)'
    }
    Emit '--- [3b] 会话生命周期行（辅证 / 降级口径） ---'
    foreach ($l in @($records | Select-String -Pattern '教师端启动|教师端停止|学员加入|学员离线|已加入|与教师机断开|旧连接')) {
        Emit $l.Line
    }
    Emit '--- [3c] 锚点解析（机器可读 TSV：kind<TAB>timestamp<TAB>item_id<TAB>file<TAB>size） ---'
    # 解析口径按 issue #14 更正：实现已 .with_target(false)，日志行**不含模块路径**，
    # 故只按「关键词 + 字段」匹配，不做整行模板匹配；容忍 tracing 的两种行内布局：
    #   A) 消息在前： <ts> INFO broadcast item_id=.. file=.. size=..
    #   B) 字段在前： <ts> INFO item_id=.. file=.. size=.. broadcast
    $tsv = @()
    foreach ($a in $anchors) {
        $line = $a.Line
        $ts = $null
        if ($line -match '(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})?)') { $ts = $Matches[1] }
        $kind = $null
        if ($line -match '(?<![\w])(broadcast|received|applied)(?![\w=])') { $kind = $Matches[1] }
        $itemId = ''
        if ($line -match '(?<![\w])item_id="?([^\s",]+)') { $itemId = $Matches[1] }
        $fileName = ''
        if ($line -match '(?<![\w])file=(.+)$') {
            # 文件名可能含空格（如「第一课 讲义.pptx」）：引号优先，否则截到下一个字段或行尾，
            # 再剔除尾部的锚点关键词（布局差异：关键词可能在字段之前或之后）
            $v = $Matches[1].Trim()
            if ($v.StartsWith('"')) {
                $end = $v.IndexOf('"', 1)
                if ($end -gt 0) { $fileName = $v.Substring(1, $end - 1) }
                else { $fileName = $v.Trim('"') }
            } else {
                $nx = [regex]::Match($v, '\s+[a-z_]+=')
                if ($nx.Success) { $fileName = $v.Substring(0, $nx.Index).Trim() } else { $fileName = $v }
                $fileName = ($fileName -replace '\s+(broadcast|received|applied)$', '').Trim()
            }
        }
        $size = ''
        if ($line -match '(?<![\w])size=(\d+)') { $size = $Matches[1] }
        $tsv += ($kind + "`t" + $ts + "`t" + $itemId + "`t" + $fileName + "`t" + $size)
    }
    if ($tsv.Count -gt 0) {
        Emit "kind`tTimestamp`titem_id`tfile`tsize"
        foreach ($r in $tsv) { Emit $r }
        Emit ("锚点条数: " + $tsv.Count)
    } else {
        Emit '(无可解析锚点)'
    }
} else {
    Emit ("日志不存在: " + $log)
}

# 4) 指定条目的落盘文件属性与哈希
if ($ItemName) {
    Emit ''
    Emit ('--- [4] 条目落盘证据: ' + $ItemName + ' ---')
    $repo = Join-Path $root 'repo'
    $hits = @()
    $baks = @()
    if (Test-Path $repo) {
        $hits = @(Get-ChildItem $repo -Recurse -File -Filter $ItemName -ErrorAction SilentlyContinue)
        $baks = @(Get-ChildItem $repo -Recurse -File -Filter ($ItemName + '.bak') -ErrorAction SilentlyContinue)
    }
    if ($hits.Count -gt 0) {
        foreach ($h in $hits) {
            Emit ($h.FullName)
            Emit ("  Length=" + $h.Length + "  CreationTime=" + $h.CreationTime.ToString('yyyy-MM-ddTHH:mm:ss.fff') + "  LastWriteTime=" + $h.LastWriteTime.ToString('yyyy-MM-ddTHH:mm:ss.fff'))
            Emit ("  SHA256=" + (Get-FileHash $h.FullName -Algorithm SHA256).Hash)
        }
    } else {
        Emit ('(未找到落盘文件 ' + $ItemName + ')')
    }
    Emit ('同名 .bak 数量: ' + $baks.Count)
    foreach ($b in $baks) {
        Emit ('  BAK ' + $b.FullName + '  SHA256=' + (Get-FileHash $b.FullName -Algorithm SHA256).Hash)
    }
}

# 5) 索引概览（ShareItem：文件名/标题/版本/删除标记/同步态）
Emit ''
Emit '--- [5] SQLite 索引概览 ---'
$db = Join-Path $root 'index.db'
if (Test-Path $db) {
    $py = Get-Command python -ErrorAction SilentlyContinue
    if ($py) {
        $pyFile = Join-Path ([IO.Path]::GetTempPath()) ('cs_index_' + [guid]::NewGuid().ToString('N') + '.py')
        $pyCode = @'
import sqlite3, sys
db = sys.argv[1]
c = sqlite3.connect(db)
try:
    rows = c.execute("select id, kind, owner_id, owner_role, audience, title, size, version, deleted, sync_state from share_items").fetchall()
    for r in rows:
        print(" | ".join("" if v is None else str(v) for v in r))
    print("索引条目数: %d" % c.execute("select count(*) from share_items").fetchone()[0])
except Exception as e:
    print("索引查询失败: %s" % e)
'@
        [IO.File]::WriteAllText($pyFile, $pyCode, (New-Object Text.UTF8Encoding($false)))
        try {
            (& python $pyFile $db 2>&1) | ForEach-Object { Emit $_ }
        } catch {
            Emit ('索引查询执行失败: ' + $_)
        } finally {
            Remove-Item $pyFile -Force -ErrorAction SilentlyContinue
        }
    } else {
        Emit '(未找到 python，无法直读 SQLite；可改用 sqlite3.exe 或随包工具)'
    }
} else {
    Emit ('索引不存在: ' + $db)
}

# 6) 仓库统计（TC-12 基线/复验对比用）
Emit ''
Emit '--- [6] 仓库统计 ---'
$repo = Join-Path $root 'repo'
if (Test-Path $repo) {
    $all = @(Get-ChildItem $repo -Recurse -File -ErrorAction SilentlyContinue)
    $sum = ($all | Measure-Object -Property Length -Sum).Sum
    Emit ('repo 文件数: ' + $all.Count + '  总字节: ' + $sum)
    foreach ($f in ($all | Select-Object -First 20)) {
        Emit ('  ' + $f.FullName + '  ' + $f.Length + ' bytes  ' + $f.LastWriteTime.ToString('yyyy-MM-ddTHH:mm:ss.fff'))
    }
} else {
    Emit ('repo 不存在: ' + $repo)
}

Emit ''
Emit ('===== 采集结束 at=' + (Get-Date -Format 'yyyy-MM-ddTHH:mm:ss.fff') + ' =====')

$text = $out -join [Environment]::NewLine
Write-Output $text
if ($OutFile) {
    $full = $OutFile
    if (-not [IO.Path]::IsPathRooted($full)) { $full = Join-Path (Get-Location) $OutFile }
    $dir = Split-Path -Parent $full
    if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
    [IO.File]::WriteAllText($full, $text, (New-Object Text.UTF8Encoding($false)))
    Write-Output ('[已写出] ' + $full)
}
