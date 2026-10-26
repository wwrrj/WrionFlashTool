# 发布新版本：生成 website/version.json（网站下载按钮和工具内检查更新都读它）
#   ./website/release.ps1 -Url https://xxx.share.123pan.cn/123pan/xxxx -Notes "更新内容 1", "更新内容 2"
#   ./website/release.ps1 -Url ... -Notes ... -MinVersion 2.1.0   # 低于 2.1.0 的版本必须更新才能继续使用
# 版本号取自 src-tauri/Cargo.toml，SHA256 取自 dist/WrionFlashTool.exe；生成后用 deploy.ps1 部署网站即可生效。
param(
    [Parameter(Mandatory = $true)][string]$Url,
    [string[]]$Notes = @(),
    [string]$MinVersion = "",
    [string]$Exe = (Join-Path $PSScriptRoot "..\dist\WrionFlashTool.exe"),
    [string]$Label = "123 云盘"
)
$ErrorActionPreference = "Stop"

if ($Url -notmatch '^https://') { throw "下载链接必须以 https:// 开头" }
if ($MinVersion -and $MinVersion -notmatch '^\d+\.\d+\.\d+$') { throw "MinVersion 必须是 x.y.z 格式" }

$Cargo = Get-Content (Join-Path $PSScriptRoot "..\src-tauri\Cargo.toml") -Raw
if ($Cargo -notmatch '(?m)^version\s*=\s*"(\d+\.\d+\.\d+)"') { throw "无法从 Cargo.toml 读取版本号" }
$Version = $Matches[1]

if (-not (Test-Path $Exe)) { throw "找不到 $Exe，请先运行 build.ps1" }
$Hash = (Get-FileHash $Exe -Algorithm SHA256).Hash.ToLower()

$Manifest = [ordered]@{
    version        = $Version
    date           = (Get-Date -Format "yyyy-MM-dd")
    notes          = @($Notes)
    download_url   = $Url
    download_label = $Label
    min_version    = $MinVersion
    files          = @([ordered]@{ name = "WrionFlashTool.exe"; label = "Windows x64"; sha256 = $Hash })
}
$Out = Join-Path $PSScriptRoot "version.json"
# 不带 BOM 的 UTF-8，浏览器和工具都能直接解析
[IO.File]::WriteAllText($Out, ($Manifest | ConvertTo-Json -Depth 4) + "`n", (New-Object Text.UTF8Encoding $false))

Write-Host "==> 已生成 version.json：v$Version  SHA256 $Hash" -ForegroundColor Green
Write-Host "    确认 123 云盘里已是这个 exe 后，运行 ./website/deploy.ps1 -Server <服务器> 发布" -ForegroundColor Yellow
