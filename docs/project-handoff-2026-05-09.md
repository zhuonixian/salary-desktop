# salary-desktop 项目交接文档

更新日期：2026-05-09  
当前仓库：`github.com:zhuonixian/salary-desktop.git`  
当前分支：`master`  
当前最新提交：`946517b fix: align desktop workflows with backend commands`  
当前发布标签：`v0.1.0` 已移动到 `946517b` 并推送，用于触发 GitHub Actions 打包。

## 最新进度记录

- 2026-08-08：用户确认第二阶段开发测试已验证通过。后续接手时，第二阶段不再按“待测试/待验证”处理；若继续开发，优先从下一阶段或新问题清单开始。

## 项目概况

- 项目名称：`salary-desktop`，工资核算桌面端。
- 技术栈：Tauri v2、React 19、TypeScript、Vite、Ant Design、SQLite/rusqlite、Rust。
- 目标平台：Windows exe/NSIS 安装包。
- Linux 本地联调建议：优先使用 `npm run tauri dev`，可以真实调用 Tauri/Rust 命令；只跑 `npm run dev` 只能验证纯前端页面。
- 打包触发方式：推送 `v*` 标签触发 `.github/workflows/build.yml` 的 `Build and Release`。

## 当前已处理的问题

### 1. Windows exe 白屏

问题已经处理到 exe 可正常访问页面。此前白屏定位过程见：

- `docs/troubleshooting-white-screen.md`

关键结论：

- 初期白屏不是 Rust 初始化失败，后端日志显示 setup 完成。
- 后续通过 HTML 诊断定位到 JS bundle/React bootstrap 问题。
- 已移除页面 debug 输出，当前界面可正常渲染。

相关历史提交：

- `aa58bad fix: handle dashboard startup blank screen`
- `08d15dc fix: remove diagnostics and align ocr rules api`

### 2. OCR 执行诊断与打包资源

已处理内容：

- Rust OCR 调用从只尝试单一 Python 改为尝试 `python`、`python3`、`py -3`。
- OCR 执行失败时输出更完整的错误信息，便于定位 Windows 环境问题。
- `src-tauri/tauri.conf.json` 已将 `../python-ocr` 加入 bundle resources。
- Rust OCR 逻辑已兼容 Python 输出 `{ success, rows, raw_text }`。

相关提交：

- `073db07 fix: improve ocr script execution diagnostics`

仍需现场验证：

- Windows 目标机是否能找到 Python。
- `python-ocr` 内依赖是否已安装。
- OCR 识别失败时新的错误详情是否足够定位。

### 3. 导出中心导出失败

用户现场报错：

```text
月度工资明细表 导出失败: invalid args `path` for command `export_salary_detail`: command export_salary_detail missing required key path
```

已修复：

- 前端导出 API 入参从 `{ savePath }` 改为后端需要的 `{ path }`。
- 工资条导出后端需要目录参数 `{ dir }`，前端已改为选择目录。
- 修复范围：
  - `export_salary_detail(month, path)`
  - `export_bank_payment_file(month, path)`
  - `export_salary_slips(month, dir)`
  - `export_attendance_summary_file(month, path)`

主要文件：

- `src/api/index.ts`
- `src/pages/ExportCenter.tsx`

相关提交：

- `946517b fix: align desktop workflows with backend commands`

### 4. 员工/考勤导入模板

用户要求：员工管理、考勤管理等需要导入 Excel 的页面提供模板下载。

已实现：

- 员工管理页面新增“下载模板”按钮。
- 考勤管理页面新增“下载模板”按钮。
- 后端新增 xlsx 模板生成命令：
  - `export_employee_import_template(path)`
  - `export_attendance_import_template(path)`
- 模板字段与现有导入解析字段保持一致。

主要文件：

- `src/pages/Employees.tsx`
- `src/pages/Attendance.tsx`
- `src-tauri/src/excel.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`

### 5. 工资计算为空但提示成功

用户问题：工资计算页面内容为空，但点击“一键计算”提示成功，需要检查前置条件。

已处理：

- 一键计算前增加校验：
  - 员工列表不能为空。
  - 必须存在在职员工。
  - 当前月份必须存在考勤数据。
  - 工资规则、个税规则必须可读取。
  - 当前月份已锁定时禁止重新计算。
- 计算结果为空时改为提示检查员工和考勤是否匹配。
- 修复前后端字段不一致导致页面为空：
  - 后端工资字段：`name`、`salary_month`、`overtime_salary`、`tax_amount` 等。
  - 前端页面字段：`employee_name`、`month`、`overtime_pay`、`income_tax` 等。
  - 已在 `src/api/index.ts` 增加 normalize 适配层。
- 修复员工状态映射：
  - 前端：`在职/离职/试用`
  - 后端：`active/inactive/probation`
  - 防止手动新增员工状态为“在职”但后端工资计算只认 `active`，导致计算结果为空。

主要文件：

- `src/api/index.ts`
- `src/pages/SalaryCalculate.tsx`

### 6. 工资复核命令缺失

此前前端调用 `review_salary`，后端没有该命令。

已处理：

- 新增后端 `review_salary_results(month)`。
- 复核只把未锁定工资结果状态改为 `reviewed`。
- 锁定仍使用 `lock_salary_results(month)`，把状态改为 `locked` 并设置 locked。
- 前端 `reviewSalary` 已改为调用 `review_salary_results`。

主要文件：

- `src-tauri/src/db.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `src/api/index.ts`

### 7. 界面布局随窗口变化比例不对

已做基础修复：

- `html/body/#root` 增加宽度和 overflow 控制。
- 主内容区域基于侧边栏宽度计算 `width`。
- `.app-main` 使用 `clamp()` 调整 margin，并启用内部滚动。
- 表格、卡片、栅格增加 `min-width: 0`，减少横向撑爆。
- 小屏时主内容按折叠侧边栏宽度适配。

主要文件：

- `src/styles/global.css`

仍需 UI 现场验证：

- Windows exe 下最大化、还原、缩放窗口时页面是否符合预期。
- 宽表格页面是否需要进一步优化列固定和横向滚动体验。

## 已验证命令

在 Linux 开发机上已通过：

```bash
npm exec tsc -b -- --pretty false
npm run build
cd src-tauri && cargo check
```

说明：

- `cargo check` 只有既有 warning：
  - `get_salary_result_by_employee` 未使用。
  - `OperationLog` 未构造。
- `npm run build` 可能出现 Vite chunk 大小提示，不影响构建。
- `npm run lint` 之前存在既有 lint 问题，不作为当前打包阻断项。

## 当前待办

### P0：新增 Linux 本地启动脚本

用户最新需求：

> 给项目增加一个启动脚本，每次启动前先确认进程是否运行，已运行则中止后再 build 启动。

建议实现：

- 新增 `scripts/start-dev.sh`。
- 在 `package.json` 增加：

```json
"start:dev": "bash scripts/start-dev.sh"
```

建议脚本流程：

1. 定位项目根目录。
2. 检查当前项目相关开发进程：
   - `vite`
   - `tauri dev`
   - `npm run dev`
   - 占用端口 `5173` 的进程
3. 如果发现已运行，先终止对应进程。
4. 执行 `npm run build`。
5. build 成功后执行 `npm run tauri dev`。

注意：

- 不要用过宽的 `pkill node`，避免误杀其他项目。
- 优先按当前项目路径、端口 `5173`、命令行关键字筛选。
- 该需求尚未落代码，因为用户随后要求先输出交接文档。

### P0：Windows 现场验证最新 exe

最新 `v0.1.0` 标签已触发打包。下个 agent 需要确认 GitHub Actions run 是否成功，并下载最新构建在 Windows 上验证：

- exe 是否仍可正常启动。
- Debug 输出是否已消失。
- 导出中心四类导出是否正常。
- 员工/考勤模板下载是否正常。
- 工资计算前置校验是否按预期提示。
- 复核、锁定流程是否正常。
- OCR 失败时是否能显示详细错误。

可用命令：

```bash
gh run list --limit 5
```

### P1：OCR 功能继续排查

用户曾报告：

```text
OCR功能操作报错: 识别失败: OCR识别错误: OCR执行失败:
```

当前已增强错误信息，但还需要 Windows 实测新的错误文本。重点检查：

- Windows 是否安装 Python。
- Python 是否能执行 `python-ocr/ocr.py`。
- OCR 所需 pip 包是否安装。
- 打包资源路径是否正确解析到 `python-ocr`。
- 图片路径中中文、空格、盘符是否影响 OCR 脚本。

相关文件：

- `src-tauri/src/ocr.rs`
- `python-ocr/`

### P1：导入/编辑考勤数据完整性

当前已做字段映射，但建议继续测试：

- Excel 导入考勤后，列表字段是否全部显示。
- 编辑考勤记录是否能保存。
- 编辑后重新拉取数据是否与数据库一致。
- `leave_days` 是前端展示字段，目前由 `personal_leave_days + sick_leave_days` 计算。

### P1：员工状态与工资计算

已增加状态映射，但建议测试：

- 新增员工默认“在职”是否写入后端 `active`。
- 编辑员工状态是否正确回写。
- 只有“在职”员工参与工资计算。
- Excel 导入员工默认后端状态为 `active`，前端显示为“在职”。

### P2：启动诊断日志清理策略

当前 Rust 仍有 `diag()` 启动日志，写入：

```text
%TEMP%\salary-desktop-startup.log
```

这对现场排查仍有价值，但发布稳定后可考虑：

- 保留但降低频率。
- 或只在 debug/诊断版本启用。

相关文件：

- `src-tauri/src/main.rs`
- `src-tauri/src/lib.rs`

### P2：Vite chunk 体积

`npm run build` 有时提示 bundle 超过 500kB。当前不阻断功能，但后续可优化：

- 路由级动态 import。
- Ant Design 按需拆分。
- Vite/Rolldown code splitting 配置。

## 本地开发建议

完整功能联调：

```bash
cd /home/zhang/workspace/Project/salary/salary-desktop
npm run tauri dev
```

纯前端布局调试：

```bash
npm run dev
```

构建校验：

```bash
npm exec tsc -b -- --pretty false
npm run build
cd src-tauri && cargo check
```

推送触发打包：

```bash
git push origin master
git tag -f v0.1.0
git push origin v0.1.0 --force
```

## 关键文件索引

- `src/api/index.ts`：前后端命令和字段适配层，当前很多功能修复集中在这里。
- `src/pages/SalaryCalculate.tsx`：工资计算页面、前置校验、复核/锁定入口。
- `src/pages/ExportCenter.tsx`：导出中心。
- `src/pages/Employees.tsx`：员工管理、员工模板下载。
- `src/pages/Attendance.tsx`：考勤管理、考勤模板下载。
- `src/styles/global.css`：全局布局和窗口缩放适配。
- `src-tauri/src/commands.rs`：Tauri command 注册函数实现。
- `src-tauri/src/lib.rs`：Tauri Builder、插件、invoke handler 注册。
- `src-tauri/src/db.rs`：SQLite 数据访问。
- `src-tauri/src/excel.rs`：Excel 导入解析、导出和模板生成。
- `src-tauri/src/ocr.rs`：OCR 执行和结果确认。
- `src-tauri/tauri.conf.json`：窗口、bundle、资源配置。

## 注意事项

- 当前工作区在写本文档前是 clean；本文档新增后需要下个 agent 决定是否提交。
- 不要随意回滚近期提交，白屏、OCR、导出、工资计算问题是连续修复链。
- 如果需要验证 Windows exe，优先看 GitHub Actions 最新 `Build and Release` 是否成功。
- 若要继续功能修复，建议先用 Linux `npm run tauri dev` 缩短反馈回路，确认后再推 tag 打包。
