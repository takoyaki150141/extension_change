param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$Root = Split-Path $PSScriptRoot -Parent
$Release = Join-Path $Root "target\release"
$DataDir = Join-Path $env:LOCALAPPDATA "ExtensionChange"
$Stage = Join-Path $DataDir "package"
$Pfx = Join-Path $DataDir "dev.pfx"
$Cer = Join-Path $DataDir "dev.cer"
$Msix = Join-Path $DataDir "ExtensionChange.msix"
$Dist = Join-Path $Root "dist"

Write-Host "Extension Change の MSI を作成します。"

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
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
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

Write-Host "パッケージを作成します。"
& $makeappx pack /d $Stage /p $Msix /o | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "makeappx に失敗しました。"
}

$cert = Get-ChildItem "Cert:\CurrentUser\My" |
    Where-Object { $_.Subject -eq "CN=ExtensionChange" } |
    Select-Object -First 1
if (-not $cert) {
    Write-Host "ビルド用の署名証明書を作成します。"
    $cert = New-SelfSignedCertificate `
        -Type Custom `
        -Subject "CN=ExtensionChange" `
        -KeyUsage DigitalSignature `
        -FriendlyName "Extension Change" `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -KeyExportPolicy Exportable `
        -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
}

$password = "extension-change"
$secure = ConvertTo-SecureString -String $password -Force -AsPlainText
Export-PfxCertificate -Cert $cert -FilePath $Pfx -Password $secure | Out-Null
Export-Certificate -Cert $cert -FilePath $Cer | Out-Null

Write-Host "パッケージに署名します。"
& $signtool sign /fd SHA256 /f $Pfx /p $password /sha1 $cert.Thumbprint $Msix | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "signtool に失敗しました。"
}
Remove-Item -LiteralPath $Pfx -Force -ErrorAction SilentlyContinue

$wix = (Get-Command "wix.exe" -ErrorAction SilentlyContinue).Source
if (-not $wix) {
    $wix = Join-Path $env:USERPROFILE ".dotnet\tools\wix.exe"
}
if (-not (Test-Path -LiteralPath $wix)) {
    throw "wix が見つかりません。dotnet tool install --global wix を実行してください。"
}

$msi = Join-Path $Dist "ExtensionChange.msi"
Remove-Item -LiteralPath $msi -Force -ErrorAction SilentlyContinue
Write-Host "MSI を作成します。"
& $wix build (Join-Path $PSScriptRoot "product.wxs") -d "MsixPath=$Msix" -d "CerPath=$Cer" -d "Version=0.1.4" -arch x64 -o $msi
if ($LASTEXITCODE -ne 0) {
    throw "wix build に失敗しました。"
}

Write-Host ""
Write-Host "MSI: $msi"
Write-Host "インストールすると、署名証明書を信頼して MSIX を登録します。"
