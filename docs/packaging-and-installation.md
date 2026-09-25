# 打包与安装运行指南（Tauri 2 项目参考）

> 本文档整理 salary-desktop 的打包体系与三平台安装运行方式，供其他 Tauri 2 项目参考引用。基于 v0.9.0 实际配置（`.github/workflows/build.yml`）与实测产物。

## 一、打包体系

### 1. 技术底座（决定体积与依赖模型）

| 层 | 选择 | 对打包的影响 |
|---|---|---|
| 前端 | React + Vite → 纯静态 dist/ | 运行时**不需要 Node**，Node 只是构建工具链 |
| 后端 | Rust 静态编译（含 SQLite C 源码，rusqlite bundled） | 单二进制，无外部运行时 |
| 渲染 | 系统 WebView（Windows: WebView2 / macOS: WKWebView / Linux: WebKitGTK） | **不打包 Chromium**，exe 仅 5.8 MB（Electron 同类约 100 MB） |

### 2. 本地打包

```bash
npm install                    # 前端依赖
npm run tauri build            # = vite build + cargo release + bundle
# 产物在 src-tauri/target/release/bundle/ 下
```

### 3. CI 自动打包（核心可复用模式）

workflow：`.github/workflows/build.yml`

- **触发**：`push: tags: 'v*'`（tag 即发版）+ `workflow_dispatch`（手动验证）
- **三平台 job 并行**：
  - `build-windows`：tag 触发（`if: github.event_name == 'push'`，dispatch 跳过防同名产物覆盖）
  - `build-macos`：tag 触发，`--target aarch64-apple-darwin --bundles dmg`
  - `build-linux`：仅 workflow_dispatch 触发
- **版本号注入**（关键技巧）：仓库内版本号只是占位，构建时 `sed` 把 tag 版本 + 构建日期写入 `Cargo.toml` 与 `tauri.conf.json`（如 `0.9.0+20260918`），实现"同版本多次构建文件名可区分"。三个坑：
  1. macOS 是 BSD sed，`-i` 必须带空后缀：`sed -i '' "s|...|...|"`
  2. 非 tag 触发（如 dispatch master）必须跳过注入，否则版本变成 `master+日期`，非合法 semver，tauri 直接拒绝
  3. Windows 想用 WiX/MSI 就得放弃 `+日期` 策略（WiX 要求 build metadata 为纯数字且 ≤65535）——本项目选 NSIS 保日期
- **发布动作**：`tauri-apps/tauri-action@v0` 自动创建 **draft release** 并上传产物 → 人工核对后 `gh release edit <tag> --draft=false --notes-file <file>` 正式发布
- **缓存**：`swatinem/rust-cache@v2`（`with: workspaces: src-tauri`）

workflow 骨架示例：

```yaml
on:
  push:
    tags: ['v*']
  workflow_dispatch:
permissions:
  contents: write
jobs:
  build-windows:
    if: ${{ github.event_name == 'push' }}
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: 22 }
      - uses: dtolnay/rust-toolchain@stable
      - uses: swatinem/rust-cache@v2
        with: { workspaces: src-tauri }
      - run: npm install
      - name: Inject build date into version   # sed 注入 tag 版本+日期
        shell: bash
        run: ...
      - uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        with:
          tagName: ${{ github.ref_name }}
          releaseName: '应用名 ${{ github.ref_name }}'
          releaseDraft: true
          prerelease: false
          args: --bundles nsis
  build-macos:
    runs-on: macos-latest
    steps:
      # ...同上，注入用 sed -i ''；加 targets: aarch64-apple-darwin
      - uses: tauri-apps/tauri-action@v0
        with:
          # ...同上
          args: --target aarch64-apple-darwin --bundles dmg
```

### 4. 产物清单（v0.9.0 实测）

| 平台 | 文件 | 大小 |
|---|---|---|
| Windows | `salary-desktop_0.9.0+20260918_x64-setup.exe`（NSIS） | 5.8 MB |
| macOS | `salary-desktop_0.9.0_aarch64.dmg`（Apple Silicon） | 8.3 MB |
| Linux | `*_amd64.deb` / `-1.x86_64.rpm` / `*_amd64.AppImage` | 8.7 / 8.7 / 79.7 MB |

## 二、安装与运行

### Windows

1. 双击 setup.exe → NSIS 向导（默认装 `C:\Users\<用户>\AppData\Local\<应用名>\`，可改目录）
2. **SmartScreen 拦截**（未购买代码签名证书的必经步骤）：蓝色"Windows 已保护你的电脑"→ 点**"更多信息"**→ 点**"仍要运行"**
3. 唯一软依赖 **WebView2**（Win10/11 一般系统自带；缺失表现为启动白屏，安装 Edge / WebView2 Runtime 即解决）
4. 桌面图标双击启动；首次启动引导初始化（如启动密码）

### macOS（Apple Silicon：M1/M2/M3/M4）

1. 双击 dmg 挂载 → 将应用图标拖入**应用程序（Applications）**→ 退出挂载
2. **Gatekeeper**（未做 Apple 公证）：首次在应用程序中对图标**右键 →「打开」→ 再点「打开」**确认一次，此后双击正常启动；直接双击被拦时同样走右键打开
3. 无任何软依赖（WKWebView 系统必带）
4. Intel 芯片 Mac：aarch64 dmg 不可用，需自行编译（Rust + Node + Xcode Command Line Tools）

### Linux

- AppImage：`chmod +x` 后直接执行
- deb/rpm：走对应包管理器
- 需要系统 WebKitGTK 运行库（主流发行版自带或一条命令安装）

### 数据目录（Tauri `app_data_dir`，identifier 决定）

```
Windows:  %APPDATA%\<identifier>\
macOS:    ~/Library/Application Support/<identifier>/
Linux:    ~/.local/share/<identifier>/
```

本项目 identifier = `com.salary.desktop`，数据库/发票归档/备份均在该目录下。

## 三、迁移到别的项目的要点清单

1. `tauri.conf.json` 定好 `identifier`（决定数据目录与产物命名），尽早确定不再改
2. 版本号三处（package.json / tauri.conf.json / Cargo.toml）**保持同步**；或采用本项目模式——仓库内只放占位、CI 从 tag 注入真实版本
3. 未签名的代价写进用户文档：Windows SmartScreen + macOS Gatekeeper 的绕过步骤（购买代码签名证书 / Apple 公证可消除，公证需 Apple 开发者账号 $99/年）
4. dispatch 验证新 CI job 必须用**包含新 workflow 的分支/ref** 触发——`--ref <旧tag>` 会执行旧版 workflow，新 job 不生效
5. Windows bundle 二选一：NSIS（支持 `+日期` 版本）或 MSI/WiX（版本受严格限制）
6. macOS 只发 aarch64 时文档写明 Intel 用户自行编译；需要双架构用 universal（体积近翻倍、构建约 2 倍时长）
7. draft release 的产物下载用 `gh api` 按 asset ID（`browser_download_url` 对 draft 可能是 untagged 链接，普通下载易卡/半截），下载后与 release digest 做 SHA256 校验
8. 大陆网络环境：CI 物料下载可用镜像加速（如 ghfast.top 前缀），下载前后均校验 SHA256
