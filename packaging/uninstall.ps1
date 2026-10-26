param(
    [switch]$Purge,
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
    if ($Purge) { $arguments += " -Purge" }
    Start-Process "powershell.exe" -ArgumentList $arguments -Verb RunAs
    exit
}

$DataDir = Join-Path $env:LOCALAPPDATA "ExtensionChange"

Get-Process -Name "ExtensionChange" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

$package = Get-AppxPackage -Name "ExtensionChange" -ErrorAction SilentlyContinue
if ($package) {
    Remove-AppxPackage -Package $package.PackageFullName
    Write-Host "パッケージの登録を解除しました。"
} else {
    Write-Host "パッケージは登録されていません。"
}

if ($Purge) {
    if (Test-Path -LiteralPath $DataDir) {
        Remove-Item -LiteralPath $DataDir -Recurse -Force
        Write-Host "設定とプラグインを削除しました: $DataDir"
    }

    Get-ChildItem "Cert:\LocalMachine\My" |
        Where-Object { $_.Subject -eq "CN=ExtensionChange" } |
        Remove-Item -Force -ErrorAction SilentlyContinue
    Get-ChildItem "Cert:\LocalMachine\TrustedPeople" |
        Where-Object { $_.Subject -eq "CN=ExtensionChange" } |
        Remove-Item -Force -ErrorAction SilentlyContinue
    Write-Host "署名証明書を削除しました。"
}
