---
name: release-workflow
description: 发版流程 — git tag 触发 GitHub Actions 构建 + gh release 发布 + SmartScreen 应对
---

# 发版流程

## 触发构建

CI：`.github/workflows/build.yml`，触发条件：
- `push: tags: v*`（推 `v` 开头的 tag）
- `workflow_dispatch`（手动，但默认 tagName=master，不创建正式 release）

历史惯例：每个修复版本打一个 tag，触发自动构建 + 创建 draft release。

## 标准发版步骤

```bash
# 1. 确认改动已 commit + push 到 master
git push origin master

# 2. 打 tag（version 由功能/严重程度定）
git tag -a v0.X.Y -m "<type>(<scope>): <短描述>"
git push origin v0.X.Y

# 3. 等 Actions 跑完（约 11 分钟，2 个并行 job）
gh run watch <run-id> --repo zhuonixian/salary-desktop
# 或网页看 https://github.com/zhuonixian/salary-desktop/actions

# 4. 构建成功后 release 是 draft 状态，publish 才公开
gh release edit v0.X.Y --repo zhuonixian/salary-desktop --draft=false
```

## 版本号约定

- `v0.1.x`：发票管理 MVP + 历次小修
- `v0.2.x`：发票模块关键 bug 修复（字段格式 / asset 协议）
- `v0.3.x`：未来大改动

二进制文件名仍是 `salary-desktop_0.1.0_x64-setup.exe`（`Cargo.toml` 的 version 没同步更新，与 tag 版本号不一致，是项目历史遗留）。

## 构建产物

5 个资产：
- Windows: `salary-desktop_0.1.0_x64-setup.exe`（NSIS）+ `_x64_en-US.msi`
- Linux: `_amd64.deb` + `_amd64.AppImage` + `-0.1.0-1.x86_64.rpm`

每个资产带 GitHub 自动生成的 SHA256 digest（`gh release view --json assets --jq '.assets[].digest'`）。

## 下载到本地 dist/

优先使用 GitHub API 按 asset ID 下载。原因：自动创建的 release 通常是 draft，`browser_download_url` 可能是 `untagged-*`，普通 `gh release download --pattern "*.exe"` 可能长时间卡住或留下半截文件。

```bash
cd /home/zhang/workspace/Project/salary/salary-desktop
# 用代理（GitHub 下载国内常超时）
export http_proxy=http://127.0.0.1:18080 https_proxy=http://127.0.0.1:18080
export HTTP_PROXY=http://127.0.0.1:18080 HTTPS_PROXY=http://127.0.0.1:18080

TAG=vX.Y.Z
ASSET_NAME=salary-desktop_0.1.0_x64-setup.exe
ASSET_ID=$(gh api repos/zhuonixian/salary-desktop/releases --paginate \
  --jq ".[] | select(.tag_name == \"$TAG\") | .assets[] | select(.name == \"$ASSET_NAME\") | .id")
EXPECTED_SHA=$(gh release view "$TAG" --repo zhuonixian/salary-desktop --json assets \
  --jq ".assets[] | select(.name == \"$ASSET_NAME\") | .digest" | sed 's/^sha256://')

gh api -H 'Accept: application/octet-stream' \
  "repos/zhuonixian/salary-desktop/releases/assets/$ASSET_ID" \
  > "dist/$ASSET_NAME.part"
sha256sum "dist/$ASSET_NAME.part"
test "$(sha256sum "dist/$ASSET_NAME.part" | awk '{print $1}')" = "$EXPECTED_SHA"
mv "dist/$ASSET_NAME.part" "dist/$ASSET_NAME"
stat -c '%n %s bytes' "dist/$ASSET_NAME"
```

普通下载方式仅作为备选，且必须校验大小和 SHA256；如果命令长时间无输出，先检查是否留下半截文件，不要直接信任：

```bash
cd /home/zhang/workspace/Project/salary/salary-desktop
export http_proxy=http://127.0.0.1:18080 https_proxy=http://127.0.0.1:18080
gh release download vX.Y.Z --repo zhuonixian/salary-desktop --pattern "*.exe" --dir dist --clobber

# 校验
sha256sum dist/*.exe
gh release view vX.Y.Z --repo zhuonixian/salary-desktop --json assets \
  --jq '.assets[] | select(.name | endswith("setup.exe")) | .digest'
```

## Release Notes 模板

之前 v0.1.5 / v0.1.6 用过模板（含功能介绍 + SmartScreen 简要指南 + 数据存储位置 + SHA256 校验命令），保存于 `.superpowers/sdd/v0.X.Y-release-notes.md`。

更新 release notes：
```bash
gh release edit vX.Y.Z --repo zhuonixian/salary-desktop \
  --notes-file /path/to/release-notes.md
```

## Windows SmartScreen 应对

应用未购买代码签名证书。首次运行 Windows setup.exe 会被 SmartScreen 红色拦截：
1. 双击 exe → 蓝色"Windows 已保护你的电脑"
2. 点 **"更多信息"**
3. 点 **"仍要运行"**
4. 安装程序正常启动

详细指南：`docs/windows-install-guide.md`，README 顶部有链接。

## GitHub Actions 已知问题

- **Node.js 20 弃用警告**：`actions/checkout@v4` 等被强制 Node 24 跑（无害）
- **不能取消单个 job**：API 不支持，只能 cancel 整个 run
- **构建时长**：Linux ~8 分钟，Windows ~10 分钟

## 错过的 codex review

发版前可选：`/codex review` 跑独立 diff review（用 `danger-full-access` 模式，因 sandbox bubblewrap 在某些机器初始化失败）。cost ~30k tokens / review。
