param(
    [switch]$Build,
    [switch]$NoElevate
)

$ErrorActionPreference = "Stop"

function Test-Admin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

if ($Build -and -not (Test-Admin)) {
    if ($NoElevate) {
        throw "ビルドする場合は管理者権限で実行してください。"
    }
    $arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`" -Build"
    Start-Process "powershell.exe" -ArgumentList $arguments -Verb RunAs
    exit
}

$Root = Split-Path $PSScriptRoot -Parent
$DataDir = Join-Path $env:LOCALAPPDATA "ExtensionChange"
$Dist = Join-Path $Root "dist"
$Release = Join-Path $Dist "release"
$Msix = Join-Path $DataDir "ExtensionChange.msix"
$Cer = Join-Path $DataDir "dev.cer"

if ($Build) {
    & (Join-Path $PSScriptRoot "install.ps1") -BuildOnly -NoElevate
    if ($LASTEXITCODE -ne 0) {
        throw "install.ps1 -BuildOnly に失敗しました。"
    }
}

if (-not (Test-Path -LiteralPath $Msix)) {
    throw "ビルド済みの MSIX がありません: $Msix"
}
if (-not (Test-Path -LiteralPath $Cer)) {
    throw "公開証明書がありません: $Cer"
}

if (Test-Path -LiteralPath $Release) {
    Remove-Item -LiteralPath $Release -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $Release | Out-Null

Copy-Item -LiteralPath $Msix -Destination (Join-Path $Release "ExtensionChange.msix") -Force
Copy-Item -LiteralPath $Cer -Destination (Join-Path $Release "ExtensionChange.cer") -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot "install_prebuilt.ps1") -Destination $Release -Force

$zip = Join-Path $Dist "ExtensionChange-0.1.4.zip"
Remove-Item -LiteralPath $zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $Release "*") -DestinationPath $zip -Force

Write-Host ""
Write-Host "配布用ファイル: $zip"
Write-Host "受け取る人は展開して install_prebuilt.ps1 を実行します。"
