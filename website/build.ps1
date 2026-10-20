# 把 frontend 构建到 website/app，供展示页嵌入真实界面（浏览器预览模式 + 演示数据）
$ErrorActionPreference = "Stop"

$AppDir = Join-Path $PSScriptRoot "app"
$Frontend = Join-Path $PSScriptRoot "..\frontend"
pnpm --dir $Frontend exec vite build --base ./ --outDir $AppDir --emptyOutDir
if ($LASTEXITCODE -ne 0) { throw "Frontend build failed" }

Write-Host "==> website/ 已就绪。本地预览：./website/serve.ps1" -ForegroundColor Green
