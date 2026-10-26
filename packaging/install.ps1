param(
    [switch]$SkipBuild,
    [switch]$NoElevate,
    [switch]$BuildOnly
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
    if ($SkipBuild) { $arguments += " -SkipBuild" }
    if ($BuildOnly) { $arguments += " -BuildOnly" }
    Start-Process "powershell.exe" -ArgumentList $arguments -Verb RunAs
    exit
}

$Root = Split-Path $PSScriptRoot -Parent
$Release = Join-Path $Root "target\release"
$DataDir = Join-Path $env:LOCALAPPDATA "ExtensionChange"
$PluginDir = Join-Path $DataDir "plugins"
$Stage = Join-Path $DataDir "package"
$Pfx = Join-Path $DataDir "dev.pfx"
$Cer = Join-Path $DataDir "dev.cer"
$Msix = Join-Path $DataDir "ExtensionChange.msix"

Write-Host "Extension Change をインストールします。"

if (-not $SkipBuild) {
    Push-Location $Root
    try {
        & cargo build --release -p extension_change_host -p extension_change_shell
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build に失敗しました。"
        }
    } finally {
        Pop-Location
    }
}

$exe = Join-Path $Release "ExtensionChange.exe"
$dll = Join-Path $Release "extension_change_shell.dll"
if (-not (Test-Path -LiteralPath $exe)) { throw "ビルド成果物が見つかりません: $exe" }
if (-not (Test-Path -LiteralPath $dll)) { throw "ビルド成果物が見つかりません: $dll" }

$sdkBin = Get-ChildItem (Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin") -Directory -ErrorAction SilentlyContinue |
    Sort-Object Name -Descending |
    ForEach-Object { Join-Path $_.FullName "x64" } |
    Where-Object { Test-Path (Join-Path $_ "makeappx.exe") } |
    Select-Object -First 1
if (-not $sdkBin) {
    throw "Windows SDK の makeappx.exe が見つかりません。Windows SDK をインストールしてください。"
}
$makeappx = Join-Path $sdkBin "makeappx.exe"
$signtool = Join-Path $sdkBin "signtool.exe"

New-Item -ItemType Directory -Force -Path $DataDir | Out-Null
New-Item -ItemType Directory -Force -Path $PluginDir | Out-Null
if (Test-Path -LiteralPath $Stage) {
    Remove-Item -LiteralPath $Stage -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $Stage | Out-Null

Copy-Item -LiteralPath $exe -Destination (Join-Path $Stage "ExtensionChange.exe") -Force
Copy-Item -LiteralPath $dll -Destination (Join-Path $Stage "shell_ext.dll") -Force

$schem = Join-Path $Root "tools\schem-convert\schem-convert.exe"
if (-not (Test-Path -LiteralPath $schem)) {
    $go = Get-Command go.exe -ErrorAction SilentlyContinue
    if ($go) {
        Push-Location (Join-Path $Root "tools\schem-convert")
        & go build -trimpath -ldflags "-s -w" -o $schem .
        if ($LASTEXITCODE -ne 0) {
            Pop-Location
            throw "go build に失敗しました。"
        }
        Pop-Location
    }
}
if (-not (Test-Path -LiteralPath $schem)) {
    throw "schem-convert.exe が見つかりません。"
}
Copy-Item -LiteralPath $schem -Destination $Stage -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot "AppxManifest.xml") -Destination (Join-Path $Stage "AppxManifest.xml") -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot "Images") -Destination $Stage -Recurse -Force
Copy-Item -LiteralPath (Join-Path $Root "plugins") -Destination $Stage -Recurse -Force

$ffmpegCandidates = @()
if ($env:EXTENSION_CHANGE_FFMPEG) { $ffmpegCandidates += $env:EXTENSION_CHANGE_FFMPEG }
$ffmpegCandidates += (Join-Path $Root "vendor\ffmpeg\ffmpeg.exe")
$ffmpegCandidates += (Join-Path $DataDir "ffmpeg\ffmpeg.exe")
$ffmpegCommand = Get-Command "ffmpeg.exe" -ErrorAction SilentlyContinue
if ($ffmpegCommand) { $ffmpegCandidates += $ffmpegCommand.Source }
$ffmpeg = $ffmpegCandidates | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -First 1
if (-not $ffmpeg) {
    throw "ffmpeg.exe が見つかりません。vendor\ffmpeg に置くか PATH に追加してください。"
}
Copy-Item -LiteralPath $ffmpeg -Destination (Join-Path $Stage "ffmpeg.exe") -Force
$ffmpegDir = Split-Path -Parent $ffmpeg
Get-ChildItem -LiteralPath $ffmpegDir -Filter "*.dll" -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match "^(av|sw)" } |
    ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $Stage -Force }

$bundledPlugins = Join-Path $Root "plugins"
if (Test-Path -LiteralPath $bundledPlugins) {
    Get-ChildItem -LiteralPath $bundledPlugins -Directory | ForEach-Object {
        $target = Join-Path $PluginDir $_.Name
        New-Item -ItemType Directory -Force -Path $target | Out-Null
        Copy-Item -Path (Join-Path $_.FullName "*") -Destination $target -Recurse -Force
    }
}

Write-Host "パッケージを作成します。"
& $makeappx pack /d $Stage /p $Msix /o | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "makeappx に失敗しました。"
}

$cert = Get-ChildItem "Cert:\LocalMachine\My" |
    Where-Object { $_.Subject -eq "CN=ExtensionChange" } |
    Select-Object -First 1
if (-not $cert) {
    Write-Host "開発用の署名証明書を作成します。"
    $cert = New-SelfSignedCertificate `
        -Type Custom `
        -Subject "CN=ExtensionChange" `
        -KeyUsage DigitalSignature `
        -FriendlyName "Extension Change" `
        -CertStoreLocation "Cert:\LocalMachine\My" `
        -KeyExportPolicy Exportable `
        -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
}

$password = "extension-change"
$secure = ConvertTo-SecureString -String $password -Force -AsPlainText
Export-PfxCertificate -Cert $cert -FilePath $Pfx -Password $secure | Out-Null
Export-Certificate -Cert $cert -FilePath $Cer | Out-Null

Write-Host "パッケージに署名します。"
& $signtool sign /fd SHA256 /f $Pfx /p $password $Msix | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "signtool に失敗しました。"
}
Remove-Item -LiteralPath $Pfx -Force -ErrorAction SilentlyContinue

if ($BuildOnly) {
    Write-Host ""
    Write-Host "ビルド完了: $Msix"
    Write-Host "証明書: $Cer"
    exit 0
}

Import-Certificate -FilePath $Cer -CertStoreLocation "Cert:\LocalMachine\TrustedPeople" | Out-Null

Get-Process -Name "ExtensionChange" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500

$old = Get-AppxPackage -Name "ExtensionChange" -ErrorAction SilentlyContinue
if ($old) {
    Write-Host "既存の登録を解除します。"
    Remove-AppxPackage -Package $old.PackageFullName
}

Write-Host "パッケージをインストールします。"
Add-AppxPackage -Path $Msix -ForceApplicationShutdown

Remove-Item -LiteralPath $Stage -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath (Join-Path $DataDir "app") -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "インストール完了。"
Write-Host "エクスプローラーでファイルを右クリックすると「変換」が表示されます。"
Write-Host "表示されない場合はエクスプローラーを再起動してください。"
Write-Host "プラグイン: $PluginDir"
