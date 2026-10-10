# Build HarusoraFlashTool.exe (Rust + Tauri v2)
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$OutDir = Join-Path $PSScriptRoot "dist"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Write-Host "==> Building HarusoraFlashTool (Rust + Tauri v2)" -ForegroundColor Cyan

# Ensure cargo and npm/node are in PATH
$CargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
$NpmBin = Join-Path $env:APPDATA "npm"
$NodeBin = "D:\Software\nodejs"
$env:PATH = "$CargoBin;$NpmBin;$NodeBin;$env:PATH"

# 1. Build frontend
Write-Host "--> Building Frontend (Vite + React)..." -ForegroundColor Yellow
pnpm --prefix frontend build
if ($LASTEXITCODE -ne 0) { throw "Frontend build failed" }

# 2. Build Tauri release binary with embedded frontend assets
Write-Host "--> Building Tauri binary with cargo tauri build..." -ForegroundColor Yellow
Push-Location "$PSScriptRoot\src-tauri"
try {
    cargo tauri build --no-bundle
    if ($LASTEXITCODE -ne 0) { throw "Tauri build failed" }
} finally {
    Pop-Location
}

$SrcExe = Join-Path $PSScriptRoot "src-tauri\target\release\harusora_flash_tool.exe"
if (-not (Test-Path $SrcExe)) {
    throw "Target binary not found: $SrcExe"
}

$DistExe = Join-Path $OutDir "HarusoraFlashTool.exe"
Copy-Item -Force $SrcExe $DistExe
Copy-Item -Force $SrcExe (Join-Path $PSScriptRoot "HarusoraFlashTool.exe")

$size = (Get-Item $DistExe).Length
Write-Host ("OK: {0} ({1:N1} MB)" -f $DistExe, ($size / 1MB)) -ForegroundColor Green
Write-Host "Run: launch HarusoraFlashTool.exe and pick a ROM .zip (or place it in an extracted ROM root)."
