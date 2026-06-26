#!/bin/bash
set -euo pipefail

echo "===== 🐳 Docker 修复脚本 ====="
echo ""

# 1. 扩容 daemon.json 地址池
echo "[1/3] 扩容 Docker 默认地址池..."
sudo cp /etc/docker/daemon.json /etc/docker/daemon.json.bak
sudo tee /etc/docker/daemon.json > /dev/null <<'EOF'
{
  "registry-mirrors": [
    "https://docker.m.daocloud.io",
    "https://dockerproxy.com",
    "https://docker.nju.edu.cn"
  ],
  "default-address-pools": [
    { "base": "10.99.0.0/16", "size": 24 },
    { "base": "10.100.0.0/16", "size": 24 },
    { "base": "10.101.0.0/16", "size": 24 },
    { "base": "10.102.0.0/16", "size": 24 }
  ]
}
EOF
echo "  ✓ 已扩容为 4 个 /16 段 = 1024 个子网"
echo "  ✓ 备份文件: /etc/docker/daemon.json.bak"

# 2. 将当前用户加入 docker 组
echo ""
echo "[2/3] 将用户 $USER 加入 docker 组..."
sudo usermod -aG docker "$USER"
echo "  ✓ 已添加"
echo "  ⚠ 需要重新登录或执行 newgrp docker 使组生效"

# 3. 重启 Docker
echo ""
echo "[3/3] 重启 Docker 服务..."
sudo systemctl restart docker
sleep 2

# 验证
if systemctl is-active --quiet docker; then
    echo "  ✓ Docker 已成功启动！"
    echo ""
    echo "===== ✅ Docker 状态 ====="
    sudo docker info --format '{{.ServerVersion}}' 2>&1 | xargs echo "  Docker 版本:"
    sudo docker network ls 2>&1 | head -5
else
    echo "  ❌ Docker 启动失败，查看日志:"
    sudo journalctl -u docker.service --no-pager -n 15
    exit 1
fi

echo ""
echo "===== 📋 后续步骤 ====="
echo "由于组权限变更需要新会话生效，请运行以下命令来构建并启动项目："
echo ""
echo "  newgrp docker"
echo "  cd /home/benny/codes/web-ssh"
echo "  docker compose up -d --build"
echo ""
echo "或者直接重新打开一个终端窗口执行上述命令。"
