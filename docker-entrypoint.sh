#!/bin/sh
# Tabnest 容器入口：准备数据目录，然后以非 root 用户启动服务。
set -eu

# 数据目录（SQLite 数据库与图标缓存都放在这里），默认 /data。
DATA_DIR="${DATA_DIR:-/data}"
mkdir -p "$DATA_DIR/favicons"

# 容器以 root 启动的唯一目的是修正 bind mount 的 ./data 权限，
# 随后立刻降权到 10001:10001 的 tabnest 用户运行主程序。
# setpriv 不可用时（chown 失败也只告警）仍会继续尝试 exec。
if [ "$(id -u)" = "0" ]; then
    chown -R 10001:10001 "$DATA_DIR" 2>/dev/null || true
    exec setpriv --reuid=10001 --regid=10001 --clear-groups /usr/local/bin/tabnest "$@"
fi

# 已经是非 root（例如 compose 里指定了 user），直接启动。
exec /usr/local/bin/tabnest "$@"
