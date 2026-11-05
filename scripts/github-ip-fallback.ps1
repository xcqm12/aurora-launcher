# GitHub 直连备用：扫描官方 IP 段并生成 hosts 条目
#
# 用途：git 推送/拉取 github.com 直连间歇失败（DNS 污染或运营商抖动）时，
# 从 GitHub 官方公布的 IP 段（https://api.github.com/meta）抽样测 TCP 443，
# 输出可用 IP 与对应的 hosts 行。api.github.com 通常可达，用它拿官方段。
#
# 用法：
#   powershell -File scripts/github-ip-fallback.ps1              # 只扫描
#   powershell -File scripts/github-ip-fallback.ps1 -WriteHosts  # 扫描并把最快的 IP 写入 hosts（需管理员）
#
# 撤销：删除 hosts 里的 "github-ip-fallback" 注释行之间的条目。

param(
    [switch]$WriteHosts
)

$ErrorActionPreference = "Stop"
# 官方段兜底（2026-11-05 取自 https://api.github.com/meta，api 限流时使用）
$fallbackSegments = @(
    "140.82.112.0/20", "192.30.252.0/22", "185.199.108.0/22",
    "143.55.64.0/20", "20.205.243.166/32", "20.201.28.151/32"
)
try {
    $meta = Invoke-RestMethod -Uri "https://api.github.com/meta" -TimeoutSec 20
    # github.com 的 HTTPS 由 web/api 段服务；排除 IPv6
    $segments = @($meta.web) + @($meta.api) | Where-Object { $_ -notmatch ':' }
    if (-not $segments) { $segments = $fallbackSegments }
} catch {
    Write-Output "api.github.com 获取官方段失败（限流或网络），使用内置兜底段。"
    $segments = $fallbackSegments
}
$candidates = New-Object System.Collections.Generic.List[string]
foreach ($segment in $segments | Select-Object -First 6) {
    if ($segment -match '^(\d+)\.(\d+)\.(\d+)\.0/(\d+)$') {
        $base = [int[]]@($Matches[1], $Matches[2], $Matches[3])
        $prefix = [int]$Matches[4]
        if ($prefix -ge 20) {
            foreach ($last in @(3, 4, 9, 10)) {
                $candidates.Add("$($base[0]).$($base[1]).$($base[2]).$last")
            }
        }
    }
}
# 常用单点 IP 直接加入
$candidates.Add("20.205.243.166")
$candidates.Add("20.201.28.151")

$alive = New-Object System.Collections.Generic.List[string]
$jobs = @()
foreach ($ip in ($candidates | Select-Object -Unique)) {
    $jobs += [pscustomobject]@{ IP = $ip }
}

# Windows PowerShell 5.1 没有 -Parallel：顺序探测，1.2s 超时足够区分
# “拒绝连接”（立刻失败）与“丢包超时”。
foreach ($job in $jobs) {
    $tcp = New-Object System.Net.Sockets.TcpClient
    try {
        $task = $tcp.ConnectAsync($job.IP, 443)
        if ($task.Wait(1200) -and $tcp.Connected) { $alive.Add($job.IP) }
    } catch {} finally { $tcp.Dispose() }
}

if ($alive.Count -eq 0) {
    Write-Output "扫描完成：官方 IP 段全部不可达（可能是本地网络整体故障，稍后重试）。"
    exit 1
}
Write-Output "TCP 443 可用 $($alive.Count) 个："
$alive | ForEach-Object { Write-Output "  $_" }

if (-not $WriteHosts) {
    Write-Output ""
    Write-Output "写入 hosts（需管理员）重跑： powershell -File scripts/github-ip-fallback.ps1 -WriteHosts"
    exit 0
}

# 取第一个可用 IP 写 hosts；保留旧备份，标记行便于撤销。
$hostsPath = "$env:SystemRoot\System32\drivers\etc\hosts"
$beginMark = "# github-ip-fallback begin"
$endMark = "# github-ip-fallback end"
$block = $beginMark + "`r`n" + $alive[0] + " github.com`r`n" + $endMark
$pattern = [regex]::Escape($beginMark) + "(?s).*?" + [regex]::Escape($endMark)
$content = Get-Content $hostsPath -Raw
if ($content -match $pattern) {
    $content = $content -replace $pattern, $block
} else {
    $content = $content.TrimEnd() + "`r`n`r`n" + $block
}
Copy-Item $hostsPath "$hostsPath.aurora-bak" -Force
Set-Content -Path $hostsPath -Value $content -Encoding ASCII
Write-Output "已把 $($alive[0]) github.com 写入 hosts（备份：hosts.aurora-bak）。撤销：删除标记行之间内容后 ipconfig /flushdns。"
