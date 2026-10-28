param(
    [Parameter(Mandatory = $true)]
    [string]$Thumbprint,
    [string]$CerPath,
    [switch]$CheckOnly
)

$ErrorActionPreference = "Stop"

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    throw "需要管理员权限：请以管理员身份启动 PowerShell 后重试"
}

$cert = Get-ChildItem Cert:\CurrentUser\My, Cert:\LocalMachine\My -ErrorAction SilentlyContinue |
    Where-Object { $_.Thumbprint -eq $Thumbprint } |
    Select-Object -First 1
if (-not $cert) {
    if ($CerPath -and (Test-Path $CerPath)) {
        $cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2((Resolve-Path $CerPath).Path)
    } else {
        throw "找不到指纹为 $Thumbprint 的证书；请用 -CerPath 指定公钥证书"
    }
}
if ($cert.Thumbprint -ne $Thumbprint) {
    throw "证书指纹不匹配：$($cert.Thumbprint)"
}

foreach ($storeName in @("Root", "TrustedPublisher")) {
    $existing = Get-ChildItem "Cert:\LocalMachine\$storeName" -ErrorAction SilentlyContinue |
        Where-Object { $_.Thumbprint -eq $Thumbprint }
    if ($existing) {
        Write-Output "已存在：LocalMachine\$storeName"
        continue
    }
    if ($CheckOnly) {
        Write-Output "缺失：LocalMachine\$storeName"
        continue
    }
    $store = New-Object System.Security.Cryptography.X509Certificates.X509Store($storeName, "LocalMachine")
    $store.Open("ReadWrite")
    $store.Add($cert)
    $store.Close()
    Write-Output "已导入：LocalMachine\$storeName"
}

Write-Output "公钥指纹：$($cert.Thumbprint)"
Write-Output "主体：$($cert.Subject)"
Write-Output "到期：$($cert.NotAfter)"
