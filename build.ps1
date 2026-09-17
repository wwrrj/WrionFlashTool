# Build lightweight standalone HarusoraFlashTool.exe
# Prefer Nuitka; fall back to PyInstaller.

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$OutDir = Join-Path $PSScriptRoot "dist"
$IconIco = Join-Path $PSScriptRoot "assets\harusora.ico"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

if (-not (Test-Path $IconIco)) {
    throw "Missing application icon: $IconIco"
}

function Test-Cmd($Name) {
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

Write-Host "==> Building HarusoraFlashTool" -ForegroundColor Cyan

# Ensure entry is importable
$env:PYTHONPATH = $PSScriptRoot

if (Test-Cmd "python") {
    $PythonExe = (Get-Command "python").Source
    $PythonPrefix = @()
} elseif (Test-Cmd "py") {
    $PythonExe = (Get-Command "py").Source
    $PythonPrefix = @("-3")
} else {
    throw "Python not found"
}

# Probe nuitka
$hasNuitka = $false
try {
    & $PythonExe @PythonPrefix -c "import nuitka" 2>$null
    if ($LASTEXITCODE -eq 0) { $hasNuitka = $true }
} catch {}

if ($hasNuitka) {
    Write-Host "Using Nuitka (onefile)..." -ForegroundColor Green
    & $PythonExe @PythonPrefix -m nuitka `
        --onefile `
        --assume-yes-for-downloads `
        --remove-output `
        --output-dir="$OutDir" `
        --output-filename=HarusoraFlashTool.exe `
        --windows-console-mode=force `
        --windows-icon-from-ico="$IconIco" `
        --product-name="HarusoraFlashTool" `
        --file-description="Harusora profile-driven Android fastboot flasher" `
        --company-name="Harusora Team" `
        --product-version=2.0.0 `
        --file-version=2.0.0 `
        --include-package=harusora_flash_tool `
        "$PSScriptRoot\launcher.py"
    if ($LASTEXITCODE -ne 0) { throw "Nuitka failed: $LASTEXITCODE" }
} else {
    Write-Host "Nuitka not installed; trying PyInstaller..." -ForegroundColor Yellow
    $hasPi = $false
    try {
        & $PythonExe @PythonPrefix -c "import PyInstaller" 2>$null
        if ($LASTEXITCODE -eq 0) { $hasPi = $true }
    } catch {}
    if (-not $hasPi) {
        Write-Host "Installing PyInstaller..." -ForegroundColor Yellow
        & $PythonExe @PythonPrefix -m pip install --upgrade pyinstaller
    }
    & $PythonExe @PythonPrefix -m PyInstaller `
        --noconfirm `
        --clean `
        --onefile `
        --console `
        --icon "$IconIco" `
        --name HarusoraFlashTool `
        --distpath "$OutDir" `
        --workpath (Join-Path $OutDir "build") `
        --specpath (Join-Path $OutDir "spec") `
        "$PSScriptRoot\launcher.py"
    if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed: $LASTEXITCODE" }
}

$exe = Join-Path $OutDir "HarusoraFlashTool.exe"
if (-not (Test-Path $exe)) {
    throw "Output exe missing: $exe"
}

# Also copy next to project root so it can be placed directly in a ROM package.
Copy-Item -Force $exe (Join-Path $PSScriptRoot "HarusoraFlashTool.exe")

$size = (Get-Item $exe).Length
Write-Host ("OK: {0} ({1:N1} MB)" -f $exe, ($size / 1MB)) -ForegroundColor Green
Write-Host "Run: place HarusoraFlashTool.exe in the ROM root and double-click it"
