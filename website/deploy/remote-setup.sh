#!/usr/bin/env bash
# 在服务器上执行（由 website/deploy.ps1 上传并调用）：
#   bash remote-setup.sh <域名> <http|https> [证书通知邮箱]
# 安装 nginx（如缺少）→ 发布网站文件 → 写入站点配置 → 可选签发 HTTPS 证书
set -euo pipefail

DOMAIN="$1"
MODE="${2:-http}"
EMAIL="${3:-}"
ROOT="/var/www/$DOMAIN"
SUDO=sudo
[ "$(id -u)" = 0 ] && SUDO=

install_pkg() {
  if command -v apt-get >/dev/null; then
    $SUDO apt-get update -y -q
    $SUDO DEBIAN_FRONTEND=noninteractive apt-get install -y -q "$@"
  elif command -v dnf >/dev/null; then
    $SUDO dnf install -y -q "$@"
  else
    echo "无法识别的系统，请先手动安装：$*" >&2
    exit 1
  fi
}

if ! command -v nginx >/dev/null; then
  echo "--> 安装 nginx"
  install_pkg nginx
  $SUDO systemctl enable --now nginx
fi

echo "--> 发布网站文件到 $ROOT"
$SUDO rm -rf "$ROOT.new"
$SUDO mkdir -p "$ROOT.new"
$SUDO tar -xzf /tmp/wrion-site.tgz -C "$ROOT.new"
$SUDO find "$ROOT.new" -type d -exec chmod 755 {} +
$SUDO find "$ROOT.new" -type f -exec chmod 644 {} +
if [ -d "$ROOT" ]; then
  $SUDO rm -rf "$ROOT.old"
  $SUDO mv "$ROOT" "$ROOT.old"
fi
$SUDO mv "$ROOT.new" "$ROOT"

# 站点配置只在第一次写入；之后 certbot 会往里加 HTTPS 配置，不能覆盖
if [ -d /etc/nginx/sites-available ]; then
  CONF="/etc/nginx/sites-available/$DOMAIN"
  LINK="/etc/nginx/sites-enabled/$DOMAIN"
else
  CONF="/etc/nginx/conf.d/$DOMAIN.conf"
  LINK=""
fi
if [ ! -f "$CONF" ]; then
  echo "--> 写入 nginx 站点配置 $CONF"
  sed -e "s#__DOMAIN__#$DOMAIN#g" -e "s#__ROOT__#$ROOT#g" /tmp/nginx-site.conf | $SUDO tee "$CONF" >/dev/null
  [ -n "$LINK" ] && $SUDO ln -sf "$CONF" "$LINK"
fi
$SUDO nginx -t
$SUDO systemctl reload nginx

if [ "$MODE" = "https" ]; then
  if ! command -v certbot >/dev/null; then
    echo "--> 安装 certbot"
    install_pkg certbot python3-certbot-nginx
  fi
  echo "--> 签发 / 续期 Let's Encrypt 证书"
  if [ -n "$EMAIL" ]; then
    $SUDO certbot --nginx -d "$DOMAIN" --non-interactive --agree-tos -m "$EMAIL" --redirect --keep-until-expiring
  else
    $SUDO certbot --nginx -d "$DOMAIN" --non-interactive --agree-tos --register-unsafely-without-email --redirect --keep-until-expiring
  fi
fi

rm -f /tmp/wrion-site.tgz /tmp/nginx-site.conf /tmp/remote-setup.sh
echo "==> 部署完成：$DOMAIN"
