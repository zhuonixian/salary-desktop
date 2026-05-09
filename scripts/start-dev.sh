#!/usr/bin/env bash
# salary-desktop 开发环境启动脚本
# 功能：检查并终止已有进程 → build → tauri dev

set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PORT=5173
LOG_PREFIX="[start-dev]"

cd "$PROJECT_DIR"

echo "$LOG_PREFIX 项目目录: $PROJECT_DIR"

# ── 终止已有进程 ──────────────────────────────────────
echo "$LOG_PREFIX 检查已有开发进程..."

# 1. 占用端口 5173 的进程
port_pids=$(lsof -ti :"$PORT" 2>/dev/null || true)
if [ -n "$port_pids" ]; then
    echo "$LOG_PREFIX 发现占用端口 $PORT 的进程: $port_pids"
    echo "$port_pids" | xargs kill -9 2>/dev/null || true
    echo "$LOG_PREFIX 已终止端口 $PORT 的进程"
    sleep 1
fi

# 2. 当前项目路径下的 vite/tauri dev 进程（避免误杀其他项目）
for cmd_pattern in "vite" "tauri"; do
    pids=$(pgrep -f "$cmd_pattern" 2>/dev/null || true)
    if [ -n "$pids" ]; then
        # 只杀 cwd 在当前项目目录下的进程
        for pid in $pids; do
            proc_cwd=$(readlink -f "/proc/$pid/cwd" 2>/dev/null || true)
            if [ "$proc_cwd" = "$PROJECT_DIR" ] || [[ "$proc_cwd" == "$PROJECT_DIR"/* ]]; then
                echo "$LOG_PREFIX 终止项目进程: PID=$pid ($cmd_pattern)"
                kill -9 "$pid" 2>/dev/null || true
            fi
        done
    fi
done

echo "$LOG_PREFIX 进程检查完毕"

# ── 构建 ──────────────────────────────────────────────
echo "$LOG_PREFIX 开始构建前端..."
if ! npm run build; then
    echo "$LOG_PREFIX 前端构建失败，终止启动"
    exit 1
fi
echo "$LOG_PREFIX 前端构建完成"

# ── 启动 ──────────────────────────────────────────────
echo "$LOG_PREFIX 启动 tauri dev..."
exec npx tauri dev
