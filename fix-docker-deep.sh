#!/bin/bash
set -euo pipefail

export SUDO_ASKPASS=/tmp/askpass.sh
SUDO="sudo -A"

echo "===== 🐳 Docker 深度修复 ====="
echo ""

# 1. 停止所有 Docker 进程
echo "[1/4] 停止 Docker 服务..."
$SUDO systemctl stop docker 2>/dev/null || true
$SUDO systemctl stop docker.socket 2>/dev/null || true
sleep 1

# 2. 清理旧的网络状态数据库
echo "[2/4] 清理旧的 Docker 网络分配记录..."
NETWORK_DB="/var/lib/docker/network/files/local-kv.db"
if $SUDO test -f "$NETWORK_DB"; then
    $SUDO mv "$NETWORK_DB" "${NETWORK_DB}.bak"
    echo "  ✓ 已备份: ${NETWORK_DB}.bak"
else
    echo "  ✓ 无需清理"
fi

# 3. 重启 Docker
echo "[3/4] 启动 Docker 服务..."
$SUDO systemctl start docker
sleep 3

# 验证
if $SUDO systemctl is-active --quiet docker; then
    echo "  ✓ Docker 已成功启动！"
    echo ""
    echo "===== ✅ Docker 状态 ====="
    $SUDO docker info --format 'Docker 版本: {{.ServerVersion}}' 2>&1
    $SUDO docker network ls 2>&1
else
    echo "  ❌ Docker 启动失败，查看日志:"
    $SUDO journalctl -u docker.service --no-pager -n 20
    exit 1
fi

echo ""
echo "===== 📋 用户组 ====="
echo "将用户 $USER 加入 docker 组（生效需要新终端）..."
$SUDO usermod -aG docker "$USER"
echo "  ✓ 已添加"

echo ""
echo "===== ✅ 完成 ====="
