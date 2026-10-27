param(
    [switch]$Build
)

$ErrorActionPreference = "Stop"

$Root = Split-Path $PSScriptRoot -Parent
$DataDir = Join-Path $env:LOCALAPPDATA "ExtensionChange"
$Dist = Join-Path $Root "dist"
$Release = Join-Path $Dist "release"
$Msix = Join-Path $DataDir "ExtensionChange.msix"
$Cer = Join-Path $DataDir "dev.cer"

if ($Build) {
    & (Join-Path $PSScriptRoot "build_prebuilt.ps1")
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
Copy-Item -LiteralPath (Join-Path $PSScriptRoot "install_prebuilt.bat") -Destination (Join-Path $Release "install.bat") -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot "register_classic.ps1") -Destination $Release -Force

$manifest = [xml](Get-Content (Join-Path $PSScriptRoot "AppxManifest.xml") -Raw)
$version = (($manifest.Package.Identity.Version -split '\.')[0..2] -join '.')
$versionFull = $manifest.Package.Identity.Version
$publisher = $manifest.Package.Identity.Publisher
$base = "https://github.com/takoyaki150141/extension_change/releases/latest/download"
$appinstaller = @"
<?xml version="1.0" encoding="utf-8"?>
<AppInstaller Uri="$base/ExtensionChange.appinstaller" Version="$versionFull" xmlns="http://schemas.microsoft.com/appx/appinstaller/2018">
  <MainPackage Name="ExtensionChange" Version="$versionFull" Publisher="$publisher" ProcessorArchitecture="x64" Uri="$base/ExtensionChange.msix" />
  <UpdateSettings>
    <OnLaunch HoursBetweenUpdateChecks="0" ShowPrompt="true" UpdateBlocksActivation="false" />
    <AutomaticBackgroundTask />
  </UpdateSettings>
</AppInstaller>
"@
$appinstallerPath = Join-Path $Release "ExtensionChange.appinstaller"
[IO.File]::WriteAllText($appinstallerPath, $appinstaller, [Text.UTF8Encoding]::new($false))
Copy-Item -LiteralPath $appinstallerPath -Destination $Dist -Force
Copy-Item -LiteralPath (Join-Path $Release "ExtensionChange.msix") -Destination $Dist -Force
Copy-Item -LiteralPath (Join-Path $Release "ExtensionChange.cer") -Destination $Dist -Force
$zip = Join-Path $Dist "ExtensionChange-$version.zip"
Remove-Item -LiteralPath $zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $Release "*") -DestinationPath $zip -Force

Write-Host ""
Write-Host "配布用ファイル: $zip"
Write-Host "受け取る人は展開して install.bat を実行します。"
