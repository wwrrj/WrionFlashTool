# 部署展示页到自己的服务器（使用 SSH 密钥登录，不会询问密码）
#   ./website/deploy.ps1 -Server azureuser@40.83.77.55
#   ./website/deploy.ps1 -Server azureuser@40.83.77.55 -Https -Email you@example.com   # 首次签发 HTTPS 证书
param(
    [Parameter(Mandatory = $true)][string]$Server,
    [string]$Domain = "flash.wwrrj.cn",
    [switch]$Https,
    [string]$Email = "",
    [switch]$SkipBuild
)
$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot

if (-not $SkipBuild) { & (Join-Path $Root "build.ps1") }

# 只打包网站本身，不带部署脚本
$Archive = Join-Path ([IO.Path]::GetTempPath()) "wrion-site.tgz"
# 固定用 Windows 自带的 tar：Git Bash 里的 GNU tar 会把 C:\ 路径当成远程主机
$Tar = Join-Path $env:SystemRoot "System32\tar.exe"
if (-not (Test-Path $Tar)) { $Tar = "tar" }
& $Tar -czf $Archive -C $Root index.html styles.css main.js favicon.svg version.json app
if ($LASTEXITCODE -ne 0) { throw "打包失败" }

# BatchMode：只用密钥登录，密钥不可用时直接报错而不是询问密码
$SshOpts = @("-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=accept-new")
Write-Host "--> 上传到 $Server" -ForegroundColor Yellow
scp @SshOpts $Archive (Join-Path $Root "deploy\nginx-site.conf") (Join-Path $Root "deploy\remote-setup.sh") "${Server}:/tmp/"
if ($LASTEXITCODE -ne 0) { throw "上传失败（检查 SSH 密钥是否已加到服务器）" }

$Mode = if ($Https) { "https" } else { "http" }
# 去掉 Windows 换行后再执行，避免 \r 导致脚本报错
ssh @SshOpts $Server "tr -d '\r' < /tmp/remote-setup.sh > /tmp/remote-setup.lf && bash /tmp/remote-setup.lf '$Domain' '$Mode' '$Email'; rc=`$?; rm -f /tmp/remote-setup.lf; exit `$rc"
if ($LASTEXITCODE -ne 0) { throw "服务器端部署失败" }

Remove-Item $Archive -ErrorAction SilentlyContinue
$Scheme = if ($Https) { "https" } else { "http" }
Write-Host "==> 已部署：${Scheme}://$Domain/" -ForegroundColor Green
