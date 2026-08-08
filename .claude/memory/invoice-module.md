---
name: invoice-module
description: 发票管理模块 — dedup key、OCR 调用流程、PDF 直传、图片归档
---

# 发票管理模块

模块位置：`src-tauri/src/invoice.rs` + `src/pages/Invoices.tsx` + `db.rs` 的 invoice 部分。

## 表结构（`database/schema.sql`）

```sql
invoice_expense_types (id, code UNIQUE, name, sort_order, enabled, remark)
invoices (
  id, invoice_code, invoice_number, invoice_type,
  issue_date, check_code,
  amount, tax_amount, total_amount,
  seller_name, seller_tax_id, buyer_name, buyer_tax_id,
  expense_type_code, employee_id, belong_month,
  status DEFAULT 'normal',   -- normal / void
  remark, image_path, raw_ocr_json,
  created_at, updated_at
)

CREATE UNIQUE INDEX idx_invoices_code_number
  ON invoices(COALESCE(invoice_code, ''), invoice_number) WHERE status != 'void';
```

## 去重 key

- **核心**：`COALESCE(invoice_code, '')` + `invoice_number`
- **支持全电票**：code 为 NULL/空 时按 number 单独去重（partial index + COALESCE 把 NULL 当 '' 处理）
- **业务层**：`find_invoice_by_dedup_key(code: Option<&str>, number: &str)` 在 db.rs
- **输入规范化**：`save_invoice` 业务层在 db 写入前 trim + lower-case code/number
- **三层去重**：DB unique index（兜底）+ 业务层 `find_invoice_by_dedup_key`（save 前查）+ 前端保存按钮 disabled（OCR 检测到 is_duplicate 时）

## OCR 调用流程

```
commands::ocr_invoice(image_path, state: &Mutex<Connection>)
    ↓
invoice::ocr_invoice(image_path, db_ops: &D)  // D: InvoiceOcrDbOps
    ├─ token = db_ops.get_baidu_access_token()  // 锁内取 token
    ├─ file_data = std::fs::read(image_path)
    ├─ HTTP POST vat_invoice  ← 不持锁
    ├─ 解析返回，遇 error_code==110 返回 InvoiceOcrInnerError::TokenInvalid
    └─ db_ops.find_invoice_by_dedup_key()  // 锁内查重
[若 TokenInvalid]
    ↓
    db_ops.clear_baidu_access_token()  // 清缓存
    重新 ocr_invoice_inner 一次
```

## 百度 vat_invoice 接口

- 端点：`https://aip.baidubce.com/rest/2.0/ocr/v1/vat_invoice`
- 参数：
  - PDF：`pdf_file=<base64>` + `pdf_file_num=1`
  - 图片：`image=<base64>`
  - 可选：`seal_tag=false`（不识别发票章）
- **重要**：返回的 `words_result` 字段值是**直接字符串**（`"InvoiceNum": "26317000002652868787"`），不是通用 OCR 的 `{"word": "..."}` 对象格式。`pick_str()` 三层兼容：对象.word / 字符串 / 数组.word。
- token：`ocr::get_baidu_access_token(conn)` 共享给考勤 OCR 使用，30 天缓存

## 图片归档

`invoice::copy_image_to_app_dir(src, belong_month, app_data_dir)`：
1. 净化 `belong_month`：仅字母数字/`-`/`_`，空则 `unclassified`
2. 净化 filename：`sanitize_invoice_filename()` 移除路径分隔符和控制字符
3. 复制到 `{app_data_dir}/invoices/{sanitized_month}/{timestamp}_{sanitized_filename}`
4. 返回完整路径字符串存入 `image_path` 列

## 业务层关键函数

- `save_invoice(input, db_ops, app_data_dir)` — guard number 非空 → 二次查重 → 复制图 → insert → log_operation
- `update_invoice(id, input, db_ops, app_data_dir)` — 检测 code/number 自我冲突 → 复制新图（如有变化）→ update → log
- `delete_invoice(id, db_ops)` — 软删除（`status='void'`）→ log

## 前端关键交互

- **上传流程**：`handleUploadClick` → 选文件 → `ocrInvoice(filePath)` → Modal 表单预填 → 选报销人/费用类型 → `handleSaveInvoice`
- **OCR 失败兜底**：保留 selectedFilePath，开空表单让手工录入
- **重复硬拦截**：`uploadModal.preview.is_duplicate` 时保存按钮 disabled
- **类型下拉**：`增值税普通发票/专用发票/电子普通发票/电子发票(普通发票)/电子发票(增值税专用发票)`
- **详情 Drawer**：用 `convertFileSrc(image_path)` 渲染原图（PDF iframe / 图片 img）

## 费用类型管理

- 7 个预设：office/travel/meal/transport/accommodation/communication/other（sort_order 1-6, 99）
- `other` 不允许删除（业务兜底）
- 被引用（`count_invoices_by_expense_type > 0`）的费用类型不允许删除，只能 disable
- 前端下拉框过滤 `enabled === 1`

## 关键测试

- `test_save_invoice_normalized_code_number_collide` — `" 12345678 "` 与 `"12345678"` 应冲突
- `test_unique_index_blocks_duplicate_no_code` — 全电票同号空码拦截
- `test_save_invoice_blocks_duplicate_full_electronic` — 业务层重复拦截
- `test_soft_delete_allows_resubmission` — 软删除后可重新报销同一张
- `test_map_vat_invoice_string_format` — 百度 vat_invoice 实际返回结构解析
