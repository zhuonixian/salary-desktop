---
name: commands-reference
description: 完整开发 / 测试 / 构建命令清单
---

# 命令清单

## 前端开发

```bash
npm install                              # 安装依赖
npm run dev                              # 仅前端 dev server（无 Tauri）
npm run build                            # 前端构建（tsc + vite build → dist/）
npm run lint                             # ESLint
npm run preview                          # 预览构建产物
npm run start:dev                        # scripts/start-dev.sh（项目自定义启动）
npx tsc -b                               # 类型检查（勿用 tsc --noEmit：根 tsconfig 仅 refs+files:[]，裸跑为空检查恒过）
```

## Tauri / 全栈

```bash
npm run tauri dev                        # 开发模式（前端热重载 + Rust 重编译）
npm run tauri build                      # 打包发布版本
npm run tauri -- <args>                  # 透传给 tauri CLI
```

## 后端 Rust（在 src-tauri/ 下执行）

```bash
cd src-tauri
cargo check                              # 编译检查
cargo build                              # 构建
cargo test --lib                         # 单元测试（252 个，第七阶段收尾口径）
cargo test --lib db::tests               # 仅 db 测试
cargo test --lib invoice                 # 仅 invoice 测试
cargo fmt                                # 格式化（codex review 修复时跑过）
cargo fix --lib -p salary-desktop        # 自动修简单 warning
```

## Python OCR Sidecar（python-ocr/）

```bash
cd python-ocr
pip install -r requirements.txt          # PaddleOCR 等依赖
python main.py --image <path> --mode attendance --output result.json
```

## 发版

```bash
# 推 master + 打 tag + 推 tag
git push origin master
git tag -a vX.Y.Z -m "<type>(<scope>): <msg>"
git push origin vX.Y.Z

# 监控构建
gh run watch <run-id> --repo zhuonixian/salary-desktop

# Publish draft release
gh release edit vX.Y.Z --repo zhuonixian/salary-desktop --draft=false

# 下载产物到本地（带代理）
cd dist
export http_proxy=http://127.0.0.1:18080 https_proxy=http://127.0.0.1:18080
gh release download vX.Y.Z --repo zhuonixian/salary-desktop --pattern "*.exe" --clobber
sha256sum *.exe  # 对比 GitHub 公布的 digest
```

## 网络 / 代理

CLAUDE.md / Bash 工具的非交互 shell 不继承 `.bashrc`。直读：

```bash
OCR_KEY=$(grep -E '^ocr_key=' ~/.bashrc | head -1 | sed -E 's/.*="([^"]+)".*/\1/')
```

代理 9674（旧）已废，现在用 18080（`systemctl --user start gost-bridge`）。GitHub 下载/推送慢时挂代理。

## superpowers/sdd 工作目录（已 gitignore）

`.superpowers/sdd/` 存 SDD 流程的 task brief、review package、self-test 脚本等临时文件。**不入 git**（`.gitignore` 已加）。每次新会话重启 SDD 时可清空。

## docs/superpowers/

设计文档（specs + plans）**入 git**。新增功能写 `docs/superpowers/specs/YYYY-MM-DD-<topic>-design.md` + `docs/superpowers/plans/YYYY-MM-DD-<topic>.md`。

## 系统级（poppler 等）

发票 OCR 现在**不需要** poppler（v0.1.8 起改用百度原生 `pdf_file` 参数）。考勤 OCR 用本地 PaddleOCR。Linux 上 `apt install poppler-utils` 仅在未来用 PDF 转图片兜底时需要。

## git 操作约定

- 分支策略：直接 master 开发（单人项目，不走 PR）
- commit message：中文为主，可中英混合，参考 conventional commits（`feat:` / `fix:` / `docs:` / `chore:`）
- 不跳过 hooks，不 `--no-verify`，不 force push
- `git add` 只加具体文件，不用 `git add .` 或 `-A`
