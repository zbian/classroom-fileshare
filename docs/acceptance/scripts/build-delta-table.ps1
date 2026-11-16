<#
.SYNOPSIS
  M1 验收 TC-02 跨节点时延（Delta）汇总脚本（qa-acceptance / issue #15）。

.DESCRIPTION
  汇总各机 collect-m1-evidence.ps1 产出的证据文件，按 item_id 把教师机的 broadcast 锚点
  与各学员机的 received/applied 锚点对齐，生成时延表与统计口径：

    Delta_applied = T(applied, 学员机) - T(broadcast, 教师机)   <- TC-02 主判据（<=5s）
    Delta_received = T(received, 学员机) - T(broadcast, 教师机)  <- 到达时延
    Delay_landing = T(applied) - T(received)                    <- 到达→落盘间隔

  跨机时间戳比对必须先校正时钟偏差：用 -Offsets 指定各学员机相对教师机的偏差（秒，正数表示
  学员机时钟偏快，需从学员机时间戳中减去）。偏差取自各机证据文件 [2] 时钟段的人工对拍结果。

  输出：CSV 时延表 + 控制台统计（最大值 / p95 / 5s 内节点数）。

  依赖：各机证据文件须由 collect-m1-evidence.ps1 生成（含 [3c] 锚点解析 TSV 段）。

.PARAMETER Teacher
  教师机证据文件路径（含 broadcast 锚点）。

.PARAMETER Students
  学员机证据文件路径，逗号分隔（文件名即节点名，取文件基名）。

.PARAMETER Offsets
  时钟偏差修正，形如 "学员A=-1.2,学员B=0.5"（秒）。未列出的节点不修正。

.PARAMETER Out
  输出 CSV 路径（默认写控制台）。

.EXAMPLE
  .\build-delta-table.ps1 -Teacher "evidence\M1\TC-02\teacher.txt" `
      -Students "evidence\M1\TC-02\stuA.txt,evidence\M1\TC-02\stuB.txt" -Out "delta.csv"

.NOTES
  只读：不修改被测数据。兼容 Windows PowerShell 5.1 与 PowerShell 7（UTF-8 BOM）。
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Teacher,
    [Parameter(Mandatory = $true)][string]$Students,
    [string]$Offsets = '',
    [string]$Out = ''
)

$ErrorActionPreference = 'Continue'

# 从证据文件里取 [3c] 锚点 TSV 段：形如 kind \t ts \t item_id \t file \t size
function Read-Anchors([string]$path) {
    $result = @()
    if (-not (Test-Path $path)) {
        Write-Warning ('证据文件不存在: ' + $path)
        return $result
    }
    foreach ($line in @(Get-Content $path -Encoding UTF8)) {
        if ($line -match '^(broadcast|received|applied)\t(.+)$') {
            $p = $line -split "`t"
            if ($p.Count -ge 5) {
                $result += [pscustomobject]@{
                    Kind    = $p[0]
                    Ts      = $p[1]
                    ItemId  = $p[2]
                    File    = $p[3]
                    Size    = $p[4]
                }
            }
        }
    }
    return $result
}

function To-Seconds([string]$ts) {
    if (-not $ts) { return $null }
    try {
        $d = [datetimeoffset]::Parse($ts, [cultureinfo]::InvariantCulture)
        return $d.ToUnixTimeMilliseconds() / 1000.0
    } catch { return $null }
}

# 偏移表：节点名 -> 修正秒数
$offsetMap = @{}
if ($Offsets) {
    foreach ($kv in ($Offsets -split ',')) {
        $p = $kv -split '='
        if ($p.Count -eq 2) { $offsetMap[$p[0].Trim()] = [double]$p[1].Trim() }
    }
}

$teacherAnchors = Read-Anchors $Teacher
if ($teacherAnchors.Count -eq 0) {
    Write-Warning '教师机证据中未解析到 broadcast 锚点 —— E4 是否已合入？或 -Since 过滤掉了？'
}
$teacherBcast = @{}
foreach ($a in $teacherAnchors) {
    if ($a.Kind -eq 'broadcast') { $teacherBcast[$a.ItemId] = $a }
}

$rows = New-Object System.Collections.Generic.List[object]
foreach ($sp in ($Students -split ',')) {
    $sp = $sp.Trim()
    if (-not $sp) { continue }
    $node = [IO.Path]::GetFileNameWithoutExtension($sp)
    $offset = 0.0
    if ($offsetMap.ContainsKey($node)) { $offset = $offsetMap[$node] }
    foreach ($a in (Read-Anchors $sp)) {
        if ($a.Kind -eq 'broadcast') { continue }
        if (-not $teacherBcast.ContainsKey($a.ItemId)) { continue }
        $tB = To-Seconds $teacherBcast[$a.ItemId].Ts
        $tA = To-Seconds $a.Ts
        if ($null -eq $tB -or $null -eq $tA) { continue }
        $tA = $tA - $offset
        $delta = $tA - $tB
        $rows.Add([pscustomobject]@{
            Node        = $node
            Kind        = $a.Kind
            ItemId      = $a.ItemId
            File        = $a.File
            Size        = $a.Size
            T_Broadcast = $teacherBcast[$a.ItemId].Ts
            T_Local     = $a.Ts
            OffsetSec   = $offset
            DeltaSec    = [math]::Round($delta, 3)
        })
    }
}

Write-Output ('===== Delta 汇总（teacher=' + [IO.Path]::GetFileName($Teacher) + '，节点=' + ($Students -split ',').Count + '） =====')
if ($rows.Count -eq 0) {
    Write-Output '无可对齐的锚点（请检查 E4 是否落地、教师机与学员机是否同一 item_id）'
    return
}

$applied = @($rows | Where-Object { $_.Kind -eq 'applied' })
$table = $rows | Sort-Object Node, Kind
$table | Format-Table -AutoSize | Out-String | Write-Output

$vals = @($applied | ForEach-Object { [double]$_.DeltaSec } | Sort-Object)
if ($vals.Count -gt 0) {
    $sorted = @($vals | Sort-Object)
    $p95Idx = [math]::Ceiling(0.95 * $sorted.Count) - 1
    if ($p95Idx -lt 0) { $p95Idx = 0 }
    $within = @($sorted | Where-Object { $_ -le 5 }).Count
    Write-Output ('applied 锚点数: ' + $sorted.Count)
    Write-Output ('最大 Delta: ' + [math]::Round(($sorted | Select-Object -Last 1), 3) + ' s')
    Write-Output ('p95 Delta: ' + [math]::Round($sorted[$p95Idx], 3) + ' s')
    Write-Output ('<=5s 节点数: ' + $within + ' / ' + $sorted.Count)
    Write-Output ('判定(TC-02 主判据, 全部节点<=5s): ' + $(if ($within -eq $sorted.Count) { 'PASS' } else { 'FAIL' }))
    Write-Output ('判定(9.1 p95<=5s): ' + $(if ($sorted[$p95Idx] -le 5) { 'PASS' } else { 'FAIL' }))
    if ($offsetMap.Count -gt 0) { Write-Output ('已应用时钟偏移修正: ' + (($offsetMap.GetEnumerator() | ForEach-Object { $_.Key + '=' + $_.Value }) -join ', ')) }
}

if ($Out) {
    $full = $Out
    if (-not [IO.Path]::IsPathRooted($full)) { $full = Join-Path (Get-Location) $Out }
    $dir = Split-Path -Parent $full
    if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
    $table | Export-Csv -Path $full -NoTypeInformation -Encoding UTF8
    Write-Output ('[已写出] ' + $full)
}
