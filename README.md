# Salary Desktop - 工资核算助手

基于 Tauri + React + TypeScript 的桌面端工资核算工具，支持员工管理、考勤导入、工资计算与导出。

## 功能特性

- **员工管理** — 导入/导出员工信息 Excel，管理基本工资、社保公积金等参数
- **考勤管理** — Excel 导入考勤数据，OCR 识别打卡考勤表（百度 API / 本地 PaddleOCR）
- **工资核算** — 自动计算个税、社保、公积金、考勤扣款，生成工资明细
- **报表导出** — 工资明细、银行代发、工资条、考勤汇总多格式 Excel 导出

## 技术栈

| 层级 | 技术 |
|------|------|
| 前端 | React 19 + TypeScript + Ant Design 6 + Vite |
| 后端 | Tauri 2 (Rust) + SQLite |
| OCR | 百度云 OCR API + 本地 Python PaddleOCR |

## 开发

```bash
# 安装前端依赖
npm install

# 开发模式
npm run tauri dev

# 构建
npm run tauri build
```

## 安装

从 [Releases](https://github.com/zhuonixian/salary-desktop/releases) 下载 Windows / Linux 安装包。

**Windows 首次运行会被 SmartScreen 拦截**（应用未购买代码签名证书），按 [Windows 首次安装指南](docs/windows-install-guide.md) 操作即可。

### OCR 配置

- **在线模式**：在应用设置中填入百度云 OCR API Key 和 Secret Key
- **本地模式**：需要安装 Python 3 及 PaddleOCR 依赖（见 `python-ocr/` 目录）

## License

[Apache License 2.0](LICENSE)
