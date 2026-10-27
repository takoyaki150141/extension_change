param(
    [switch]$NoElevate
)

$ErrorActionPreference = "Stop"

function Test-Admin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

if (-not (Test-Admin)) {
    if ($NoElevate) {
        throw "管理者権限で実行してください。"
    }
    $arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`""
    $process = Start-Process "powershell.exe" -ArgumentList $arguments -Verb RunAs -Wait -PassThru
    exit $process.ExitCode
}

$msix = Join-Path $PSScriptRoot "ExtensionChange.msix"
$cer = Join-Path $PSScriptRoot "ExtensionChange.cer"

if (-not (Test-Path -LiteralPath $msix)) {
    throw "ExtensionChange.msix が見つかりません: $msix"
}
if (-not (Test-Path -LiteralPath $cer)) {
    throw "ExtensionChange.cer が見つかりません: $cer"
}

Get-Process -Name "ExtensionChange" -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500

Import-Certificate -FilePath $cer -CertStoreLocation "Cert:\LocalMachine\TrustedPeople" |
    Out-Null

$old = Get-AppxPackage -Name "ExtensionChange" -ErrorAction SilentlyContinue
if ($old) {
    Remove-AppxPackage -Package $old.PackageFullName
}

Add-AppxPackage -Path $msix -ForceApplicationShutdown

$installed = Get-AppxPackage -Name "ExtensionChange" -ErrorAction SilentlyContinue
if ($installed) {
    $shellDll = Join-Path $installed.InstallLocation "shell_ext.dll"
    if (Test-Path -LiteralPath $shellDll) {
        try {
            & (Join-Path $PSScriptRoot "register_classic.ps1") -DllPath $shellDll
        } catch {
            Write-Warning "クラシック右クリックメニューを登録できませんでした: $_"
        }
    }
}

Write-Host ""
Write-Host "インストール完了。"
Write-Host "エクスプローラーを再起動すると、右クリックメニューが反映されます。"
