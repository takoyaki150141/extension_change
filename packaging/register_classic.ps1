param(
    [Parameter(Mandatory = $true)]
    [string]$DllPath
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $DllPath)) {
    throw "shell_ext.dll not found: $DllPath"
}

$clsid = "{1f3d8c2a-9e4b-4b7a-8c2d-6e5f4a3b2c10}"
$handler = "HKCU\Software\Classes\*\shellex\ContextMenuHandlers\ExtensionChange"
$class = "HKCU\Software\Classes\CLSID\$clsid"
$inproc = "$class\InprocServer32"

& reg.exe add $handler /ve /d $clsid /f | Out-Null
if ($LASTEXITCODE -ne 0) { throw "failed to register classic context menu" }

& reg.exe add $class /ve /d "Extension Change" /f | Out-Null
if ($LASTEXITCODE -ne 0) { throw "failed to register COM class" }

& reg.exe add $inproc /ve /d $DllPath /f | Out-Null
if ($LASTEXITCODE -ne 0) { throw "failed to register shell_ext.dll" }

& reg.exe add $inproc /v ThreadingModel /d Apartment /f | Out-Null
if ($LASTEXITCODE -ne 0) { throw "failed to set threading model" }
