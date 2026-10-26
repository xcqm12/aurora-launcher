$ErrorActionPreference = "Stop"

Add-Type -TypeDefinition @"
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class AuroraDpapiRead
{
    [StructLayout(LayoutKind.Sequential)]
    private struct DATA_BLOB
    {
        public int cbData;
        public IntPtr pbData;
    }

    [DllImport("Crypt32.dll", SetLastError = true)]
    private static extern bool CryptUnprotectData(
        ref DATA_BLOB pDataIn,
        IntPtr ppszDataDescr,
        IntPtr pOptionalEntropy,
        IntPtr pvReserved,
        IntPtr pPromptStruct,
        int dwFlags,
        ref DATA_BLOB pDataOut);

    [DllImport("Kernel32.dll")]
    private static extern IntPtr LocalFree(IntPtr hMem);

    public static string Unprotect(byte[] data)
    {
        DATA_BLOB input = new DATA_BLOB();
        input.pbData = Marshal.AllocHGlobal(data.Length);
        input.cbData = data.Length;
        Marshal.Copy(data, 0, input.pbData, data.Length);

        DATA_BLOB output = new DATA_BLOB();
        bool ok = CryptUnprotectData(ref input, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, 1, ref output);
        Marshal.FreeHGlobal(input.pbData);
        if (!ok)
        {
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }

        byte[] result = new byte[output.cbData];
        Marshal.Copy(output.pbData, result, 0, output.cbData);
        LocalFree(output.pbData);
        return System.Text.Encoding.UTF8.GetString(result);
    }
}
"@

$path = Join-Path (Join-Path $env:APPDATA "com.aurora.launcher") "curseforge.key"
if (-not (Test-Path $path)) {
    Write-Output "status=missing_key_file"
    exit 1
}

$key = [AuroraDpapiRead]::Unprotect([IO.File]::ReadAllBytes($path))
$headers = @{ "x-api-key" = $key }
try {
    $response = Invoke-RestMethod -Uri "https://api.curseforge.com/v1/mods/search?gameId=432&searchFilter=jei&pageSize=1" -Headers $headers -TimeoutSec 30
    $count = @($response.data).Count
    Write-Output ("status=ok results=" + $count)
}
catch {
    $code = "unknown"
    if ($_.Exception.Response) {
        $code = [int]$_.Exception.Response.StatusCode
    }
    Write-Output ("status=error http=" + $code)
    exit 1
}
