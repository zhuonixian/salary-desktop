# Windows 本机构建指南

## 1. 安装前置依赖

### 1.1 安装 Node.js (v22+)
下载地址: https://nodejs.org/
选择 LTS 版本，安装时勾选 "Add to PATH"

### 1.2 安装 Rust
下载地址: https://rustup.rs/
运行 rustup-init.exe，选择默认安装 (MSVC toolchain)

### 1.3 安装 Visual Studio Build Tools
下载地址: https://visualstudio.microsoft.com/visual-cpp-build-tools/
安装时勾选:
  - "使用 C++ 的桌面开发"
  - Windows 10/11 SDK
  - MSVC v143 生成工具

### 1.4 WebView2 (Windows 10/11 通常已内置)
如缺少，下载地址: https://developer.microsoft.com/en-us/microsoft-edge/webview2/

## 2. 构建项目

```powershell
# 打开 PowerShell 或 CMD
cd salary-desktop

# 安装前端依赖
npm install

# 开发模式运行
npx tauri dev

# 生产构建（生成安装包）
npx tauri build
```

## 3. 构建产物位置

构建完成后，安装包在:
  src-tauri/target/release/bundle/

  msi/salary-desktop_0.1.0_x64_en-US.msi    # MSI 安装包
  nsis/salary-desktop_0.1.0_x64-setup.exe    # EXE 安装包

## 4. Python OCR Sidecar 部署

### 4.1 安装 Python 3.10+
下载地址: https://www.python.org/downloads/

### 4.2 安装 OCR 依赖
```powershell
cd python-ocr
pip install -r requirements.txt
```

### 4.3 打包 Python 为独立 EXE (可选)
```powershell
pip install pyinstaller
pyinstaller --onefile --name salary-ocr main.py
# 将 dist/salary-ocr.exe 放到 Tauri 应用目录
```

## 5. 完整打包（含 OCR）

将以下内容打包为分发包:
  salary-desktop_0.1.0_x64-setup.exe    # 主程序安装包
  python-ocr/                           # OCR 模块目录
  README.md                             # 使用说明
