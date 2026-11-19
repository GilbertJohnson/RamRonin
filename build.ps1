# Builds the single-file RamRonin.exe into .\dist and prints its SHA-256.
# Usage:  powershell -ExecutionPolicy Bypass -File .\build.ps1
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item target\release\RamRonin.exe dist\RamRonin.exe -Force
Copy-Item packs-examples -Destination dist\packs-examples -Recurse -Force

$exe = Get-Item dist\RamRonin.exe
$hash = (Get-FileHash $exe -Algorithm SHA256).Hash
"{0}  {1}" -f $hash, $exe.Name | Set-Content dist\RamRonin.exe.sha256 -Encoding ascii
"Built {0} ({1:N0} KB)" -f $exe.FullName, ($exe.Length / 1KB)
"SHA-256 {0}" -f $hash

# Optional code signing (removes the SmartScreen "unknown publisher" warning).
# Set RAMRONIN_PFX to a .pfx path and RAMRONIN_PFX_PASS to its password.
if ($env:RAMRONIN_PFX -and (Get-Command signtool -ErrorAction SilentlyContinue)) {
    signtool sign /f $env:RAMRONIN_PFX /p $env:RAMRONIN_PFX_PASS /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 dist\RamRonin.exe
}
