param(
    [Parameter(Mandatory = $true)]
    [string]$Key
)

$ErrorActionPreference = "Stop"

Add-Type -TypeDefinition @"
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class AuroraDpapi
{
    [StructLayout(LayoutKind.Sequential)]
    private struct DATA_BLOB
    {
        public int cbData;
        public IntPtr pbData;
    }

    [DllImport("Crypt32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern bool CryptProtectData(
        ref DATA_BLOB pDataIn,
        string szDataDescr,
        IntPtr pOptionalEntropy,
        IntPtr pvReserved,
        IntPtr pPromptStruct,
        int dwFlags,
        ref DATA_BLOB pDataOut);

    [DllImport("Kernel32.dll")]
    private static extern IntPtr LocalFree(IntPtr hMem);

    public static byte[] Protect(string text)
    {
        byte[] bytes = System.Text.Encoding.UTF8.GetBytes(text);
        DATA_BLOB input = new DATA_BLOB();
        input.pbData = Marshal.AllocHGlobal(bytes.Length);
        input.cbData = bytes.Length;
        Marshal.Copy(bytes, 0, input.pbData, bytes.Length);

        DATA_BLOB output = new DATA_BLOB();
        bool ok = CryptProtectData(ref input, null, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, 1, ref output);
        Marshal.FreeHGlobal(input.pbData);
        if (!ok)
        {
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }

        byte[] result = new byte[output.cbData];
        Marshal.Copy(output.pbData, result, 0, output.cbData);
        LocalFree(output.pbData);
        return result;
    }
}
"@

$directory = Join-Path $env:APPDATA "com.aurora.launcher"
New-Item -ItemType Directory -Force -Path $directory | Out-Null
$target = Join-Path $directory "curseforge.key"
$encrypted = [AuroraDpapi]::Protect($Key.Trim())
[IO.File]::WriteAllBytes($target, $encrypted)
Write-Output ("Protected CurseForge key written to " + $target + " (" + $encrypted.Length + " bytes).")
