param(
    [string]$OutDir = (Join-Path $PSScriptRoot "..\src-tauri\resources\driver")
)

$ErrorActionPreference = "Stop"

$FileName = "USBip-0.9.8.1-x64.exe"
$Url = "https://github.com/vadimgrn/usbip-win2/releases/download/v.0.9.8.1/$FileName"
$ExpectedSha256 = "38CAD6D4432B52D5BB9409D9AD03B72FDFFC4ADA4CD3A48FBECA1A2752A8518A"

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$DestPath = Join-Path $OutDir $FileName

function Test-Hash([string]$Path) {
    if (-not (Test-Path $Path)) { return $false }
    $actual = (Get-FileHash -Path $Path -Algorithm SHA256).Hash.ToUpperInvariant()
    return $actual -eq $ExpectedSha256
}

if (Test-Hash $DestPath) {
    Write-Host "Verified cached $FileName (SHA256 matches)."
    exit 0
}

# Reuse optional env-configured cache directory if available
if ($env:MEMEBOARD_DRIVER_CACHE) {
    $ScratchCopy = Join-Path $env:MEMEBOARD_DRIVER_CACHE $FileName
    if (Test-Hash $ScratchCopy) {
        Copy-Item -Path $ScratchCopy -Destination $DestPath -Force
        Write-Host "Copied and verified $FileName from local cache."
        exit 0
    }
}

Write-Host "Downloading $FileName from vadimgrn/usbip-win2 v.0.9.8.1..."
$TempPath = "$DestPath.part"
Invoke-WebRequest -Uri $Url -OutFile $TempPath

if (-not (Test-Hash $TempPath)) {
    $actual = (Get-FileHash -Path $TempPath -Algorithm SHA256).Hash.ToUpperInvariant()
    Remove-Item -Force -ErrorAction SilentlyContinue $TempPath
    throw "SHA256 mismatch for $FileName! Expected $ExpectedSha256, got $actual"
}

Move-Item -Path $TempPath -Destination $DestPath -Force
Write-Host "Downloaded and verified $FileName."
