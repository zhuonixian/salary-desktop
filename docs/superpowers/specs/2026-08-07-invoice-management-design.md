# 发票管理模块设计文档

- 创建日期：2026-08-07
- 类型：新增功能
- 影响范围：前端菜单、后端模块、数据库 schema、Excel 导出

## 1. 背景与目标

工资核算桌面工具已上线运行。出纳在日常报销场景中需要核对员工提交的发票，存在以下痛点：

- 重复报销：同一张发票被同一员工或不同员工在不同月份重复提交，难以人工发现。
- 归档困难：纸质/电子发票散落各处，无统一台账。
- 费用统计：月度费用类型分布需要手工汇总。

本模块的目标：

1. 调用百度 OCR **增值税发票**接口，自动识别发票关键字段。
2. 通过「发票代码 + 发票号码」唯一索引，硬拦截重复报销。
3. 提供多维度归类（发票类型 / 费用类型 / 报销人 / 归属月份）。
4. 支持发票清单 Excel 导出，便于财务对账。

**与工资计算完全解耦**：本模块仅用于报销凭证去重与台账，不参与工资计算，不影响 `salary_monthly_results`。

## 2. 决策摘要

| 维度 | 选择 |
|------|------|
| 发票类型范围 | 增值税普通发票 / 专用发票 / 电子普通发票（百度 `vat_invoice` 接口） |
| 归类维度 | 多维度（发票类型、费用类型、报销人、归属月份） |
| 报销人来源 | 手工从 `employees` 表选择 |
| 去重策略 | 发票代码 + 发票号码组合唯一（DB 唯一索引 + 业务层查重） |
| 费用类型 | 预设 7 类（办公/差旅/餐饮/交通/住宿/通讯/其他），可由用户增删改 |
| 图片存储 | 复制到应用数据目录 `invoices/yyyy-mm/`，DB 只存路径 |
| 工资关联 | 完全独立，无任何耦合 |
| 导出 | Excel 清单导出（按当前筛选条件） |
| 重复处理 | 硬拦截：保存阶段返回 `AppError`，OCR 阶段软提醒 |

## 3. 架构与模块边界

### 后端

```
src-tauri/src/
├── ocr.rs          (改造) → get_baidu_access_token 改为 pub(crate)，发票模块复用
├── invoice.rs      (新增) → 发票业务核心：OCR 调用、字段映射、查重、归类、CRUD
├── commands.rs     (扩展) → 新增 8 个 invoice_* tauri::command
├── db.rs           (扩展) → 新增 invoices / invoice_expense_types 的 CRUD 函数
├── models.rs       (扩展) → 新增 Invoice / InvoiceInput / InvoiceOcrPreview 等
├── excel.rs        (扩展) → 新增 export_invoice_list 函数
└── lib.rs          (扩展) → 注册新命令到 tauri::generate_handler!
```

### 前端

```
src/
├── pages/Invoices.tsx       (新增) → 发票管理主页：列表+筛选+上传+编辑+导出
├── api/index.ts             (扩展) → 新增 invoice* API 函数
├── types/index.ts           (扩展) → 新增 Invoice / InvoiceInput / ExpenseType 类型
└── App.tsx                  (扩展) → 菜单和路由各 +1 项
```

### 关键边界

- `ocr.rs` 保持「OCR 入口」职责，仅暴露 token 缓存函数给 `invoice.rs`。
- `invoice.rs` 单独承担发票业务，所有解析、查重、归类逻辑内聚。
- `commands.rs` 仅做参数转发，不写业务逻辑。
- 发票表不写 `salary_month`，不参与 `salary::calculate_monthly_salary`。

### 百度 OCR 接口

- 端点：`https://aip.baidubce.com/rest/2.0/ocr/v1/vat_invoice`
- 返回结构化 JSON（非通用文本），无需自己写正则解析。
- 复用 `ocr.rs` 已有的 token 缓存机制（提前 1 天刷新）。

## 4. 数据库设计

### 新增表

```sql
CREATE TABLE IF NOT EXISTS invoice_expense_types (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  code TEXT UNIQUE NOT NULL,
  name TEXT NOT NULL,
  sort_order INTEGER DEFAULT 0,
  enabled INTEGER DEFAULT 1,
  remark TEXT
);

CREATE TABLE IF NOT EXISTS invoices (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  invoice_code TEXT,
  invoice_number TEXT,
  invoice_type TEXT,
  issue_date TEXT,
  check_code TEXT,
  amount REAL DEFAULT 0,
  tax_amount REAL DEFAULT 0,
  total_amount REAL DEFAULT 0,
  seller_name TEXT,
  seller_tax_id TEXT,
  buyer_name TEXT,
  buyer_tax_id TEXT,
  expense_type_code TEXT,
  employee_id INTEGER,
  belong_month TEXT,
  status TEXT DEFAULT 'normal',
  remark TEXT,
  image_path TEXT,
  raw_ocr_json TEXT,
  created_at TEXT,
  updated_at TEXT,
  FOREIGN KEY (employee_id) REFERENCES employees(id) ON DELETE SET NULL,
  FOREIGN KEY (expense_type_code) REFERENCES invoice_expense_types(code) ON DELETE SET NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_invoices_code_number
  ON invoices(invoice_code, invoice_number);
CREATE INDEX IF NOT EXISTS idx_invoices_employee ON invoices(employee_id);
CREATE INDEX IF NOT EXISTS idx_invoices_month ON invoices(belong_month);
CREATE INDEX IF NOT EXISTS idx_invoices_expense_type ON invoices(expense_type_code);
```

### 默认数据

```sql
INSERT OR IGNORE INTO invoice_expense_types (code, name, sort_order) VALUES
  ('office',         '办公费',   1),
  ('travel',         '差旅费',   2),
  ('meal',           '餐饮费',   3),
  ('transport',      '交通费',   4),
  ('accommodation',  '住宿费',   5),
  ('communication',  '通讯费',   6),
  ('other',          '其他',     99);
```

### 关键设计决策

1. **唯一索引** `idx_invoices_code_number`：DB 层兜底去重，业务层 + DB 层双保险。
2. **`expense_type_code` 用 code 而非 id**：跨机器导入导出仍能匹配；改名不破坏历史。
3. **`employee_id` SET NULL 删除策略**：员工被删时发票记录保留追溯凭证。
4. **`raw_ocr_json` 完整保留**：百度返回的额外字段（商品明细、税率等）保留，便于未来扩展。
5. **不建 `invoice_batches` 表**：一张发票 = 一张图 = 一条记录，无批次概念。
6. **`belong_month` 与 `issue_date` 分开**：开票日期 vs 报销月份可能不同。
7. **`status='void'` 软删除**：保留凭证痕迹，物理删除会丢失追溯能力。

## 5. 后端实现

### 5.1 models.rs 新增结构体

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceExpenseType {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub sort_order: i32,
    pub enabled: i32,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    pub id: i64,
    pub invoice_code: Option<String>,
    pub invoice_number: Option<String>,
    pub invoice_type: Option<String>,
    pub issue_date: Option<String>,
    pub check_code: Option<String>,
    pub amount: f64,
    pub tax_amount: f64,
    pub total_amount: f64,
    pub seller_name: Option<String>,
    pub seller_tax_id: Option<String>,
    pub buyer_name: Option<String>,
    pub buyer_tax_id: Option<String>,
    pub expense_type_code: Option<String>,
    pub employee_id: Option<i64>,
    pub belong_month: Option<String>,
    pub status: Option<String>,
    pub remark: Option<String>,
    pub image_path: Option<String>,
    pub raw_ocr_json: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceOcrPreview {
    pub invoice_code: Option<String>,
    pub invoice_number: Option<String>,
    pub invoice_type: Option<String>,
    pub issue_date: Option<String>,
    pub check_code: Option<String>,
    pub amount: f64,
    pub tax_amount: f64,
    pub total_amount: f64,
    pub seller_name: Option<String>,
    pub seller_tax_id: Option<String>,
    pub buyer_name: Option<String>,
    pub buyer_tax_id: Option<String>,
    pub raw_ocr_json: String,
    pub warnings: Vec<String>,
    pub is_duplicate: bool,
    pub duplicate_invoice_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceInput {
    pub invoice_code: Option<String>,
    pub invoice_number: Option<String>,
    pub invoice_type: Option<String>,
    pub issue_date: Option<String>,
    pub check_code: Option<String>,
    pub amount: Option<f64>,
    pub tax_amount: Option<f64>,
    pub total_amount: Option<f64>,
    pub seller_name: Option<String>,
    pub seller_tax_id: Option<String>,
    pub buyer_name: Option<String>,
    pub buyer_tax_id: Option<String>,
    pub expense_type_code: Option<String>,
    pub employee_id: Option<i64>,
    pub belong_month: Option<String>,
    pub remark: Option<String>,
    pub image_path: Option<String>,
    pub raw_ocr_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InvoiceQuery {
    pub belong_month: Option<String>,
    pub employee_id: Option<i64>,
    pub expense_type_code: Option<String>,
    pub invoice_type: Option<String>,
    pub keyword: Option<String>,
    pub status: Option<String>,
}
```

### 5.2 commands.rs 新增 8 个命令

```rust
#[tauri::command]
pub fn get_invoice_expense_types(state) -> Result<Vec<InvoiceExpenseType>, AppError>;

#[tauri::command]
pub fn save_invoice_expense_type(data, state) -> Result<InvoiceExpenseType, AppError>;

#[tauri::command]
pub fn delete_invoice_expense_type(id, state) -> Result<bool, AppError>;

#[tauri::command]
pub fn ocr_invoice(image_path: String, app: AppHandle, state)
    -> Result<InvoiceOcrPreview, AppError>;

#[tauri::command]
pub fn save_invoice(data: InvoiceInput, state) -> Result<Invoice, AppError>;

#[tauri::command]
pub fn update_invoice(id: i64, data: InvoiceInput, state) -> Result<bool, AppError>;

#[tauri::command]
pub fn delete_invoice(id: i64, state) -> Result<bool, AppError>;

#[tauri::command]
pub fn query_invoices(query: InvoiceQuery, state) -> Result<Vec<Invoice>, AppError>;

#[tauri::command]
pub fn export_invoice_list(query: InvoiceQuery, path: String, state) -> Result<bool, AppError>;
```

### 5.3 invoice.rs 核心流程

```rust
const BAIDU_VAT_INVOICE_URL: &str =
    "https://aip.baidubce.com/rest/2.0/ocr/v1/vat_invoice";

pub fn ocr_invoice(image_path: &str, conn: &Connection) -> AppResult<InvoiceOcrPreview> {
    let image_data = std::fs::read(image_path)?;
    let image_b64 = base64::encode(&image_data);
    let token = crate::ocr::get_baidu_access_token(conn)?;

    let resp: BaiduVatInvoiceResponse = reqwest::blocking::Client::new()
        .post(format!("{BAIDU_VAT_INVOICE_URL}?access_token={token}"))
        .form(&[("image", image_b64.as_str())])
        .send()?.json()?;

    if let Some(code) = resp.error_code { return Err(...); }

    let mut preview = map_baidu_response(&resp);

    if let (Some(c), Some(n)) = (&preview.invoice_code, &preview.invoice_number) {
        if let Some(existing) = db::find_invoice_by_code_number(conn, c, n)? {
            preview.is_duplicate = true;
            preview.duplicate_invoice_id = Some(existing.id);
            preview.warnings.push(format!(
                "该发票已存在于系统（ID={}，录入时间={}）",
                existing.id, existing.created_at.unwrap_or_default()
            ));
        }
    } else {
        preview.warnings.push("未能识别发票代码或号码，需手工补全");
    }

    Ok(preview)
}

pub fn save_invoice(input: &InvoiceInput, conn: &Connection) -> AppResult<Invoice> {
    if let (Some(c), Some(n)) = (input.invoice_code.as_ref(), input.invoice_number.as_ref()) {
        if let Some(existing) = db::find_invoice_by_code_number(conn, c, n)? {
            return Err(AppError::General(format!(
                "发票已存在：代码{c} 号码{n}，记录ID={}",
                existing.id
            )));
        }
    }

    let target_path = copy_image_to_app_dir(&input.image_path, &input.belong_month)?;
    let invoice = db::insert_invoice(conn, input, &target_path)?;

    db::log_operation(conn, "save_invoice",
        &format!("录入发票：{} {} 金额{:.2}",
            input.invoice_code.as_deref().unwrap_or(""),
            input.invoice_number.as_deref().unwrap_or(""),
            input.total_amount.unwrap_or(0.0)),
        "system", None)?;
    Ok(invoice)
}
```

### 5.4 OCR 调用流程

```
用户选图
   │
   ▼
commands::ocr_invoice(image_path)
   │
   ▼
invoice::ocr_invoice
   ├─ fs::read(image_path) → base64
   ├─ ocr::get_baidu_access_token(conn)        ← 复用已有 token 缓存
   ├─ POST vat_invoice
   ├─ map_baidu_response → InvoiceOcrPreview
   └─ db::find_invoice_by_code_number → 置 is_duplicate
   │
   ▼
返回前端，用户编辑/补全/选择报销人/费用类型
   │
   ▼
commands::save_invoice(InvoiceInput)
   │
   ▼
invoice::save_invoice
   ├─ db::find_invoice_by_code_number  ← 二次查重（硬拦截）
   ├─ copy_image_to_app_dir(invoices/yyyy-mm/xxx.pdf)
   ├─ db::insert_invoice
   └─ db::log_operation
   │
   ▼
列表刷新
```

### 5.5 关键决策

1. **OCR 阶段不入库**：仅返回预览，与考勤 OCR 一致。
2. **OCR 阶段就查重**：让用户在编辑时看到"已报过"，避免填完表单被打回。
3. **save_invoice 再次查重**：防止 OCR 后用户改字段仍保存成功。
4. **图片复制时机放在 save 而非 ocr**：失败/放弃时不留垃圾。
5. **图片存储目录**：通过 `app.path().app_data_dir()` 获取，子目录结构 `invoices/{belong_month}/{timestamp}_{原始文件名}`。`timestamp` 前缀防止同名文件覆盖。`belong_month` 缺省时用 `unclassified`。
6. **纯手工新增支持**：上传 Modal 提供「跳过 OCR 直接录入」按钮，跳过 `ocr_invoice` 调用，直接展示空表单让用户填写。OCR 调用失败时也走这条路径（保留选中的文件用于复制）。所有字段都可手工编辑，发票代码+号码仍是必填且唯一。
7. **`commands.rs` 签名说明**：5.2 节的命令签名是简化伪代码（省略 `state: tauri::State<'_, Mutex<Connection>>` 等模板参数），实现时按现有 `commands.rs` 中的命令风格补全。

## 6. 前端实现

### 6.1 路由与菜单

`App.tsx`：
- 菜单新增 `{ key: '/invoices', label: '发票管理', icon: <FileTextOutlined /> }`，位置在「导出中心」前。
- 路由 `<Route path="/invoices" element={<Invoices />} />`。

### 6.2 页面布局

```
┌─────────────────────────────────────────────────────────────┐
│ [上传发票] [导出清单] [费用类型管理]      归属月份[2026-08 ▼] │
├─────────────────────────────────────────────────────────────┤
│ 筛选：报销人[全部▼] 费用类型[全部▼] 发票类型[全部▼] 关键词[__]│
├──────────┬──────────────────────────────────────────────────┤
│ 汇总卡片 │  发票列表（表格）                                  │
│ ─────── │  代码/号码 | 类型 | 开票日 | 销售方 | 报销人 | 费用 │
│ 本月张数 │            | 价税合计 | 操作(查看/编辑/删除)        │
│ 本月金额 │                                                  │
│ 重复拦截 │                                                  │
└──────────┴──────────────────────────────────────────────────┘
```

### 6.3 上传与识别 Modal

```
点[上传发票] → 文件选择器（accept=.pdf,.jpg,.png,.jpeg）
   │
   ├─ 若文件 > 10MB：报错终止
   │
   ▼ ocr_invoice(filePath)
   │  失败/识别为空时：不报错，进入空表单
   │
   ▼
Modal 切换为表单：
┌────────────────────────────────────────────┐
│ 左侧：原图预览（pdf用<iframe>，图片用<img>）│
│ 右侧：表单字段                              │
│   发票代码 [____]  发票号码 [____]          │
│   发票类型 [____]  开票日期 [____]          │
│   金额/税额/价税合计 [____]                 │
│   销售方/购买方 [____]                      │
│   报销人 [员工下拉▼]    ← 必填              │
│   费用类型 [预设下拉▼]  ← 必填              │
│   归属月份 [2026-08]   ← 默认全局月份       │
│   备注 [_____________]                      │
│                                             │
│ ⚠️ 警告区（如有）：                          │
│   "该发票已存在（ID=12，2026-07-15录入）"   │
│                                             │
│           [取消]  [保存]                    │
└────────────────────────────────────────────┘
```

### 6.4 关键交互

- **重复时禁止保存**：警告区显示后，`[保存]` 按钮 disabled（硬拦截）。
- **OCR 缺字段**：表单字段允许手工补全，发票代码+号码仍为空则保存按钮 disabled。
- **可改 OCR 结果**：所有字段可编辑（OCR 可能 6/8 混淆）。
- **重复行高亮**：`is_duplicate` 行背景浅红 + 重复徽章（仅历史脏数据兜底）。
- **查看详情**：抽屉显示完整字段 + 原图。
- **删除**：软删除（status='void'）+ log。
- **批量导出**：按当前筛选条件导出全部（不限于当前页）。

### 6.5 费用类型管理（Drawer）

- 增删改查预设类型。
- 「在用」不允许删除（仅 disable）。
- 「其他」(code='other') 不允许删除。

### 6.6 前端 API

```ts
export async function getInvoiceExpenseTypes(): Promise<InvoiceExpenseType[]>;
export async function saveInvoiceExpenseType(data: InvoiceExpenseTypeInput): Promise<InvoiceExpenseType>;
export async function deleteInvoiceExpenseType(id: number): Promise<void>;

export async function ocrInvoice(filePath: string): Promise<InvoiceOcrPreview>;
export async function saveInvoice(data: InvoiceInput): Promise<Invoice>;
export async function updateInvoice(id: number, data: InvoiceInput): Promise<Invoice>;
export async function deleteInvoice(id: number): Promise<void>;
export async function queryInvoices(query: InvoiceQuery): Promise<Invoice[]>;
export async function exportInvoiceList(query: InvoiceQuery, savePath: string): Promise<void>;
```

`src/types/index.ts` 新增对应接口，后端字段 snake_case → 前端 camelCase 标准化，与现有代码风格保持一致。

## 7. 错误处理与边界

| 场景 | 处理方式 |
|------|----------|
| 百度 access_token 过期 | 复用 `ocr.rs` 已有逻辑：到期前 1 天自动刷新；刷新失败提示"检查 API Key" |
| 百度 `vat_invoice` 返回 `error_code` | `18` → "QPS 超限，稍后再试"；`216201` → "图片不存在或格式错误"；`216202` → "图片模糊，无法识别"；其他 → 显示原始错误 |
| 图片无法识别发票（空字段） | 不报错，返回 `warnings`，让用户手工录入 |
| 发票代码/号码重复 | OCR 阶段：返回 `is_duplicate=true` + 警告；保存阶段：`AppError` 硬拦截 + 已存记录 ID |
| 图片复制失败（磁盘满/权限） | `AppError::Io` 友好提示，不入库 |
| 删除时被引用 | 发票 `status='void'` 软删；费用类型「在用」不允许删 |
| PDF 大于百度限制（10MB） | 前端选完文件先 size 检查，超限直接报错不发请求 |
| 网络中断 | `reqwest::Error` → 提示"网络异常"，不缓存 token |
| OCR 字段类型转换失败（金额解析） | fallback 到 0，加 warning 让用户校对 |
| 全局月份切换 | 列表自动重查；不影响上传 Modal 的归属月份默认值 |

## 8. 测试策略

### Rust 单元测试（`#[cfg(test)]`）

- `map_baidu_response`：用样例 JSON 验证字段映射。
- `parse_amount`：千分位、负数、空值等边界。
- `copy_image_to_app_dir`：临时目录验证图片确实被复制、路径格式正确。

### Rust 集成测试（in-memory SQLite）

- `save_invoice` 重复保存应返回 `AppError`。
- `query_invoices` 按 month/employee/expense_type/keyword 组合筛选正确。
- `delete_invoice` 软删除后查询应过滤 void 记录。
- `find_invoice_by_code_number` 命中/不命中两条路径。

### 手工验收测试（必跑）

- 准备 2 张真实增值税发票图（1 普票、1 专票）+ 1 张测试重复报销。
- 流程：上传 → 识别 → 选报销人 → 保存 → 列表查看 → 编辑 → 重复上传拦截 → 导出 Excel → 检查导出内容。
- 覆盖 OCR 成功、部分缺字段、完全失败、重复硬拦截 4 种场景。

### 回归测试

- 现有考勤 OCR / 打卡表 OCR 流程不动，确认 `get_baidu_access_token` 改 `pub(crate)` 后 `ocr.rs` 仍编译。

### 不做的测试

- 不 mock 百度 API 做端到端测试，token 配置和环境敏感；只用真实样例 JSON 测试解析层。
- 不做性能测试：单张识别 < 5s，可接受。

## 9. 不在本期范围

- 交通票（火车票/机票/出租车票）、餐饮定额发票等非增值税发票。
- 图片 pHash 双重去重。
- 发票与工资条联动（如自动生成报销补助）。
- 多用户/多公司账套。
- 发票验真（调用百度的发票验真接口）。

## 10. 风险与缓解

| 风险 | 缓解 |
|------|------|
| 百度 API 配额耗尽 | 监控 QPS；保存失败时不丢字段，让用户重试 |
| OCR 识别精度不足 | 所有字段可编辑；提供"识别失败手工录入"入口 |
| 用户手工改了号码导致漏拦截 | DB 唯一索引兜底，并报错 |
| 历史脏数据有重复 | 列表 `is_duplicate` 兜底展示，运营人工清理 |
| 图片体积膨胀 | 默认存原图；未来可加压缩选项 |
