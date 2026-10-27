param(
    [Parameter(Mandatory = $true)]
    [string[]]$Path,
    [string]$Pfx,
    [string]$PfxPassword,
    [string]$Thumbprint,
    [string]$TimestampUrl = "http://timestamp.digicert.com"
)

$ErrorActionPreference = "Stop"

$signtool = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending |
    Select-Object -First 1
if (-not $signtool) {
    throw "找不到 signtool.exe，请安装 Windows SDK"
}
if (-not $Pfx -and -not $Thumbprint) {
    throw "需要 -Pfx 或 -Thumbprint"
}

foreach ($file in $Path) {
    if (-not (Test-Path $file)) {
        throw "文件不存在：$file"
    }
    $arguments = @("sign", "/fd", "sha256")
    if ($Pfx) {
        $arguments += @("/f", $Pfx, "/p", $PfxPassword)
    } else {
        $arguments += @("/sha1", $Thumbprint)
    }
    if ($TimestampUrl) {
        $arguments += @("/tr", $TimestampUrl, "/td", "sha256")
    }
    $arguments += $file
    & $signtool.FullName @arguments
    if ($LASTEXITCODE -ne 0) {
        throw "签名失败：$file"
    }
    & $signtool.FullName verify /pa $file
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "已签名但证书链不受公开信任：$file"
    } else {
        Write-Output "签名验证通过：$file"
    }
}
