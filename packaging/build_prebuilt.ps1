$ErrorActionPreference = 'Stop'

$root = Split-Path $PSScriptRoot -Parent
$out = Join-Path $env:LOCALAPPDATA 'ExtensionChange'
$stage = Join-Path $out 'stage'
$msix = Join-Path $out 'ExtensionChange.msix'
$cer = Join-Path $out 'dev.cer'

$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Directory -ErrorAction SilentlyContinue |
    Where-Object { Test-Path (Join-Path $_.FullName 'x64\makeappx.exe') } |
    Sort-Object Name -Descending |
    Select-Object -First 1
if (-not $sdk) { throw 'Windows SDK 10 not found.' }
$makeappx = Join-Path $sdk.FullName 'x64\makeappx.exe'
$signtool = Join-Path $sdk.FullName 'x64\signtool.exe'

$cargo = (Get-Command cargo.exe -ErrorAction SilentlyContinue).Source
if (-not $cargo) { $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe' }
if (-not (Test-Path $cargo)) { throw 'cargo.exe not found.' }

& $cargo build --manifest-path (Join-Path $root 'Cargo.toml') --release -p extension_change_host -p extension_change_shell
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed.' }

New-Item -ItemType Directory -Force -Path $out | Out-Null
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Copy-Item (Join-Path $root 'target\release\ExtensionChange.exe') $stage
Copy-Item (Join-Path $root 'target\release\extension_change_shell.dll') $stage
Copy-Item (Join-Path $root 'packaging\AppxManifest.xml') $stage
Copy-Item (Join-Path $root 'packaging\Images') $stage -Recurse
Copy-Item (Join-Path $root 'plugins') $stage -Recurse
Copy-Item (Join-Path $root 'tools\schem-convert\schem-convert.exe') $stage
Copy-Item 'C:\bin\ffmpeg.exe' $stage
Get-ChildItem 'C:\bin' -Filter 'av*.dll' | Copy-Item -Destination $stage
Get-ChildItem 'C:\bin' -Filter 'sw*.dll' | Copy-Item -Destination $stage

if (Test-Path $msix) { Remove-Item $msix -Force }
& $makeappx pack /d $stage /p $msix /o
if ($LASTEXITCODE -ne 0) { throw 'makeappx failed.' }

$cert = Get-ChildItem Cert:\CurrentUser\My |
    Where-Object { $_.Subject -eq 'CN=ExtensionChange' -and $_.HasPrivateKey } |
    Sort-Object NotAfter -Descending |
    Select-Object -First 1
if (-not $cert) {
    $cert = New-SelfSignedCertificate `
        -Type CodeSigningCert `
        -Subject 'CN=ExtensionChange' `
        -CertStoreLocation Cert:\CurrentUser\My `
        -KeyExportPolicy Exportable `
        -KeyUsage DigitalSignature `
        -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3') `
        -NotAfter (Get-Date).AddYears(3)
}

& $signtool sign /fd SHA256 /sha1 $cert.Thumbprint /d 'Extension Change' $msix
if ($LASTEXITCODE -ne 0) {
    $password = [guid]::NewGuid().ToString()
    $secure = ConvertTo-SecureString $password -AsPlainText -Force
    $pfx = Join-Path $out 'build.pfx'
    Export-PfxCertificate -Cert $cert -FilePath $pfx -Password $secure | Out-Null
    & $signtool sign /fd SHA256 /f $pfx /p $password /d 'Extension Change' $msix
    $signExit = $LASTEXITCODE
    Remove-Item $pfx -Force
    if ($signExit -ne 0) { throw 'signtool failed.' }
}

Export-Certificate -Cert $cert -FilePath $cer -Type CERT | Out-Null

Get-Item $msix, $cer | Select-Object Name, Length, LastWriteTime
Write-Host "MSIX: $msix"
Write-Host "CER:  $cer"
