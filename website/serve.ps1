# 本地预览展示页：http://127.0.0.1:5180/（在任意目录运行都可以）
param([int]$Port = 5180)
$ErrorActionPreference = "Stop"

if (-not (Test-Path (Join-Path $PSScriptRoot "app\index.html"))) {
    Write-Host "--> 未找到 website/app，先构建界面…" -ForegroundColor Yellow
    & (Join-Path $PSScriptRoot "build.ps1")
}

# 用 npx.cmd 而不是 npx.ps1：后者会把参数当成 PowerShell 参数解析，导致目录参数丢失
Push-Location $PSScriptRoot
try {
    npx.cmd --yes http-server . -a 127.0.0.1 -p $Port -c-1
} finally {
    Pop-Location
}
