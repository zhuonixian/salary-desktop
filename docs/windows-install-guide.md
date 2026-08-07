# Windows 首次安装指南

工资核算助手当前未购买代码签名证书。Windows SmartScreen 会在首次运行时弹窗警告，这是正常现象，按以下步骤绕过即可。

## 下载哪个文件

| 文件 | 适用 |
|------|------|
| `salary-desktop_0.1.0_x64-setup.exe` | 推荐 — 标准 NSIS 安装包，双击安装 |
| `salary-desktop_0.1.0_x64_en-US.msi` | 备选 — MSI 安装包，适合企业组策略分发 |

下载后建议用 PowerShell 校验文件 SHA256（可选）：

```powershell
Get-FileHash .\salary-desktop_0.1.0_x64-setup.exe -Algorithm SHA256
```

将输出的 hash 与 GitHub Release 页面公布的 hash 对照（如公布）。

## 首次运行：绕过 SmartScreen

### Windows 10 / 11 弹窗示例

双击安装包后，会看到蓝色窗口：

```
┌──────────────────────────────────────────┐
│  Windows 已保护你的电脑                   │
│                                          │
│  Windows Defender SmartScreen 阻止了     │
│  一个无法识别的应用启动。运行此应用可能   │
│  会危害你的电脑。                         │
│                                          │
│  [更多信息]   [不运行]                    │
└──────────────────────────────────────────┘
```

### 操作步骤

1. 点击 **"更多信息"** —— 不要点"不运行"
2. 窗口会变成：

```
┌──────────────────────────────────────────┐
│  Windows 已保护你的电脑                   │
│                                          │
│  应用: salary-desktop_0.1.0_x64-setup.exe │
│  发布者: 未知发布者                       │
│  (未上传至 Windows SmartScreen 服务器)   │
│                                          │
│  [不运行]   [仍要运行]                    │
└──────────────────────────────────────────┘
```

3. 点击 **"仍要运行"** —— 安装程序会正常启动

### 安装后首次启动

安装完成后，从开始菜单启动"工资核算助手"。如果再次出现 SmartScreen，重复上述步骤。**只会在首次启动时出现**，之后 Windows 会记住信任决策。

## 安装后启动又被拦（Windows 11 23H2+）

部分 Win11 较新版本会显示更严格的警告："Microsoft Defender SmartScreen 认为此应用可能具有潜在危害"。处理方式相同：**更多信息 → 仍要运行**。

## 为什么会这样？

- 应用**未购买代码签名证书**（OV/EV 证书约 ¥1500-3000/年）
- 没有 Microsoft 签名背书，Windows 无法验证发布者身份
- SmartScreen 通过"声誉系统"判断：未签名 + 下载量少 = 高风险，自动拦截
- 这是行业普遍现象，**不是病毒**。源码公开在 GitHub，可自行构建验证

## 如果完全不放心

可以**自己构建**（需要 Windows + Node.js + Rust）：

```powershell
git clone https://github.com/zhuonixian/salary-desktop.git
cd salary-desktop
npm install
npm run tauri build
```

构建产物在 `src-tauri/target/release/bundle/` 下，未经签名但完全可信（自己编译的）。

## 常见问题

**Q：能不能以后买证书直接签？**
A：可以。最经济的方案是 Azure Trusted Signing（~$10/月），可在 GitHub Actions 中集成。详见 `.github/workflows/build.yml`。

**Q：安装后卸载怎么操作？**
A：控制面板 → 程序和功能 → 找到 `salary-desktop` → 卸载。

**Q：数据存在哪里？**
A：`%APPDATA%\com.salary.desktop\` —— 包含 SQLite 数据库 `salary.db` 和发票图片目录 `invoices\`。卸载不会删除数据。
