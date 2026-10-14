//! 第十二阶段：应收台账与回款核销（spec 2026-10-13-stage12 4.2/4.3）
//!
//! 模型镜像 advance_settlement_links：收款单（settled 的 receipt）↔ 销项发票
//! 多对多部分核销，active 合计为已核销额，取消留痕恢复（月结保护）。
//! 核销不生成新凭证——收款单已按 counter_account 入账（借银行/贷应收），
//! 核销只做台账勾稽；销项登记时的收入确认凭证在 accounting 层（sales_invoice）。

use chrono::NaiveDate;
use rusqlite::{params, Connection};

use crate::cashier::CurrentOperatorState;
use crate::errors::{AppError, AppResult};
use crate::models::{
    ArAgingSummaryRow, ArAllocationCancelInput, ArAllocationInput, ArLedgerQuery, ArLedgerRow,
    ArReceiptAllocation,
};

const AMOUNT_TOLERANCE: f64 = 0.005;
/// 应收账龄提醒阈值（天）：与仪表盘提醒卡口径一致
pub const AR_AGING_REMINDER_DAYS: i64 = 90;

/// 发票当前 active 核销合计。
pub fn invoice_settled_amount(conn: &Connection, invoice_id: i64) -> AppResult<f64> {
    let v: f64 = conn.query_row(
        "SELECT COALESCE(SUM(allocated_amount), 0) FROM ar_receipt_allocations
         WHERE invoice_id = ?1 AND status = 'active'",
        params![invoice_id],
        |r| r.get(0),
    )?;
    Ok(v)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// 创建核销分摊：收款单（settled 的 receipt）↔ 销项发票（output/normal）。
/// 门禁：双方月份开放、金额不超过发票未核销余额、收款单未被冲正作废。
pub fn create_ar_allocation(
    conn: &Connection,
    current: &CurrentOperatorState,
    input: &ArAllocationInput,
) -> AppResult<ArReceiptAllocation> {
    if input.amount <= 0.0 {
        return Err(AppError::InvalidParam("核销金额必须大于 0".into()));
    }
    let (doc_month, doc_type, doc_status): (String, String, String) = conn
        .query_row(
            "SELECT belong_month, document_type, status FROM fund_documents WHERE id = ?1",
            params![input.receipt_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| AppError::NotFound(format!("收款单ID={}未找到", input.receipt_id)))?;
    if doc_type != "receipt" {
        return Err(AppError::InvalidParam(format!(
            "单据ID={}是「{}」，核销仅限收款单",
            input.receipt_id,
            crate::cashier::fund_document_type_label(&doc_type)
        )));
    }
    if doc_status != "settled" {
        return Err(AppError::InvalidParam(
            "仅已结算的收款单可核销（冲正/作废单据不可用）".into(),
        ));
    }
    crate::db::ensure_month_open(conn, &doc_month)?;

    let (inv_month, inv_direction, inv_status, inv_total, inv_no): (
        Option<String>,
        String,
        String,
        f64,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT belong_month, IFNULL(direction, 'input'), status, total_amount, invoice_number
         FROM invoices WHERE id = ?1",
            params![input.invoice_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .map_err(|_| AppError::NotFound(format!("发票ID={}未找到", input.invoice_id)))?;
    if inv_direction != "output" {
        return Err(AppError::InvalidParam(format!(
            "发票 {} 为进项发票，应收核销仅限销项发票",
            inv_no.unwrap_or_else(|| input.invoice_id.to_string())
        )));
    }
    if inv_status == "void" {
        return Err(AppError::InvalidParam(format!(
            "发票 {} 已作废，不能核销",
            inv_no.unwrap_or_else(|| input.invoice_id.to_string())
        )));
    }
    if let Some(month) = inv_month.as_deref() {
        crate::db::ensure_month_open(conn, month)?;
    }

    let settled = invoice_settled_amount(conn, input.invoice_id)?;
    let remaining = inv_total - settled;
    if input.amount - remaining > AMOUNT_TOLERANCE {
        return Err(AppError::InvalidParam(format!(
            "核销金额 {:.2} 超过发票未核销余额 {:.2}",
            input.amount, remaining
        )));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let operator = crate::cashier::current_operator_name(conn, current);
    conn.execute(
        "INSERT INTO ar_receipt_allocations
            (invoice_id, receipt_id, allocated_amount, status, remark, created_by, created_at)
         VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6)",
        params![
            input.invoice_id,
            input.receipt_id,
            input.amount,
            input.remark,
            operator,
            now
        ],
    )?;
    let id = conn.last_insert_rowid();
    crate::db::log_operation(
        conn,
        "create_ar_allocation",
        &format!(
            "应收核销：收款单#{} ↔ 销项发票 {}，核销 {:.2}",
            input.receipt_id,
            inv_no.unwrap_or_else(|| input.invoice_id.to_string()),
            input.amount
        ),
        &operator,
        Some(&format!("allocation_id={id}")),
    )?;
    get_ar_allocation(conn, id)
}

fn row_to_allocation(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArReceiptAllocation> {
    Ok(ArReceiptAllocation {
        id: row.get(0)?,
        invoice_id: row.get(1)?,
        receipt_id: row.get(2)?,
        allocated_amount: row.get(3)?,
        status: row.get(4)?,
        remark: row.get(5)?,
        created_by: row.get(6)?,
        created_at: row.get(7)?,
        cancelled_at: row.get(8)?,
        cancel_reason: row.get(9)?,
        receipt_document_no: row.get(10)?,
        receipt_settled_at: row.get(11)?,
        receipt_summary: row.get(12)?,
    })
}

const AR_ALLOC_SELECT: &str = "a.id, a.invoice_id, a.receipt_id, a.allocated_amount, a.status, \
     a.remark, a.created_by, a.created_at, a.cancelled_at, a.cancel_reason, \
     f.document_no, f.settled_at, f.summary";

pub fn get_ar_allocation(conn: &Connection, id: i64) -> AppResult<ArReceiptAllocation> {
    conn.query_row(
        &format!(
            "SELECT {AR_ALLOC_SELECT} FROM ar_receipt_allocations a \
             LEFT JOIN fund_documents f ON f.id = a.receipt_id WHERE a.id = ?1"
        ),
        params![id],
        row_to_allocation,
    )
    .map_err(|_| AppError::NotFound(format!("核销记录ID={id}未找到")))
}

/// 按发票或收款单列核销记录（active 在前，时间倒序）。
pub fn get_ar_allocations(
    conn: &Connection,
    invoice_id: Option<i64>,
    receipt_id: Option<i64>,
) -> AppResult<Vec<ArReceiptAllocation>> {
    let mut sql = format!(
        "SELECT {AR_ALLOC_SELECT} FROM ar_receipt_allocations a \
         LEFT JOIN fund_documents f ON f.id = a.receipt_id"
    );
    let mut conds = Vec::new();
    if invoice_id.is_some() {
        conds.push("a.invoice_id = ?1".to_string());
    }
    if receipt_id.is_some() {
        conds.push(format!(
            "a.receipt_id = ?{}",
            if invoice_id.is_some() { 2 } else { 1 }
        ));
    }
    if !conds.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conds.join(" AND "));
    }
    sql.push_str(" ORDER BY a.status = 'active' DESC, a.id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = if let (Some(inv), Some(rec)) = (invoice_id, receipt_id) {
        stmt.query_map(params![inv, rec], row_to_allocation)?
            .collect::<Result<Vec<_>, _>>()?
    } else if let Some(inv) = invoice_id {
        stmt.query_map(params![inv], row_to_allocation)?
            .collect::<Result<Vec<_>, _>>()?
    } else if let Some(rec) = receipt_id {
        stmt.query_map(params![rec], row_to_allocation)?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        return Err(AppError::InvalidParam(
            "须指定发票或收款单之一查询核销记录".into(),
        ));
    };
    Ok(rows)
}

/// 取消核销：active → cancelled（原因必填），双方月份须开放（月结保护）。
pub fn cancel_ar_allocation(
    conn: &Connection,
    current: &CurrentOperatorState,
    input: &ArAllocationCancelInput,
) -> AppResult<bool> {
    let reason = input.reason.trim();
    if reason.is_empty() {
        return Err(AppError::InvalidParam("取消原因必填".into()));
    }
    let alloc = conn
        .query_row(
            "SELECT id, invoice_id, receipt_id, status FROM ar_receipt_allocations WHERE id = ?1",
            params![input.id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .map_err(|_| AppError::NotFound(format!("核销记录ID={}未找到", input.id)))?;
    if alloc.3 != "active" {
        return Err(AppError::InvalidParam(format!(
            "核销记录ID={}已是「{}」，仅 active 可取消",
            input.id,
            if alloc.3 == "cancelled" {
                "已取消"
            } else {
                &alloc.3
            }
        )));
    }
    // 月结保护：发票归属月与收款单归属月均须开放
    let inv_month: Option<String> = conn.query_row(
        "SELECT belong_month FROM invoices WHERE id = ?1",
        params![alloc.1],
        |r| r.get(0),
    )?;
    if let Some(m) = inv_month.as_deref() {
        crate::db::ensure_month_open(conn, m)?;
    }
    let doc_month: String = conn.query_row(
        "SELECT belong_month FROM fund_documents WHERE id = ?1",
        params![alloc.2],
        |r| r.get(0),
    )?;
    crate::db::ensure_month_open(conn, &doc_month)?;

    let now = chrono::Utc::now().to_rfc3339();
    let operator = crate::cashier::current_operator_name(conn, current);
    let n = conn.execute(
        "UPDATE ar_receipt_allocations
         SET status = 'cancelled', cancelled_at = ?1, cancel_reason = ?2 WHERE id = ?3",
        params![now, reason, input.id],
    )?;
    if n > 0 {
        crate::db::log_operation(
            conn,
            "cancel_ar_allocation",
            &format!("取消应收核销记录#{}：{reason}", input.id),
            &operator,
            None,
        )?;
    }
    Ok(n > 0)
}

fn aging_bucket(days: i64) -> &'static str {
    match days {
        i64::MIN..=30 => "0-30",
        31..=60 => "31-60",
        61..=90 => "61-90",
        91..=180 => "91-180",
        _ => "180+",
    }
}

/// 应收台账：销项发票（非 void）维度聚合核销进度与账龄。
/// 账龄锚点：开票日（缺省取归属月 1 日）；基准日缺省今天。
pub fn get_ar_ledger(conn: &Connection, q: &ArLedgerQuery) -> AppResult<Vec<ArLedgerRow>> {
    let today = match q.as_of_date.as_deref() {
        Some(s) if !s.trim().is_empty() => NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
            .map_err(|_| AppError::InvalidParam(format!("基准日格式应为 YYYY-MM-DD：{s}")))?,
        _ => chrono::Local::now().date_naive(),
    };

    let mut sql = String::from(
        "SELECT i.id, i.invoice_code, i.invoice_number, i.issue_date, i.belong_month, \
                i.buyer_name, i.partner_id, COALESCE(b.name, i.buyer_name), \
                i.amount, i.tax_amount, i.total_amount, \
                (SELECT COALESCE(SUM(a.allocated_amount), 0) FROM ar_receipt_allocations a \
                 WHERE a.invoice_id = i.id AND a.status = 'active'), \
                COALESCE(NULLIF(i.issue_date, ''), i.belong_month || '-01')
         FROM invoices i
         LEFT JOIN business_partners b ON b.id = i.partner_id
         WHERE i.status != 'void' AND IFNULL(i.direction, 'input') = 'output'",
    );
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;
    if let Some(pid) = q.partner_id {
        sql.push_str(&format!(" AND i.partner_id = ?{idx}"));
        params_vec.push(Box::new(pid));
        idx += 1;
    }
    if let Some(kw) = q
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        sql.push_str(&format!(
            " AND (i.buyer_name LIKE ?{idx} OR i.invoice_number LIKE ?{idx} OR i.remark LIKE ?{idx})"
        ));
        params_vec.push(Box::new(format!("%{kw}%")));
        idx += 1;
    }
    if q.only_outstanding.unwrap_or(false) {
        sql.push_str(&format!(
            " AND i.total_amount - (SELECT COALESCE(SUM(a.allocated_amount), 0) \
              FROM ar_receipt_allocations a WHERE a.invoice_id = i.id AND a.status = 'active') > {AMOUNT_TOLERANCE}"
        ));
    }
    sql.push_str(" ORDER BY COALESCE(NULLIF(i.issue_date, ''), i.belong_month || '-01'), i.id");

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let raw = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, f64>(8)?,
                row.get::<_, f64>(9)?,
                row.get::<_, f64>(10)?,
                row.get::<_, f64>(11)?,
                row.get::<_, String>(12)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut rows = Vec::with_capacity(raw.len());
    for (id, code, number, issue, month, buyer, pid, pname, amount, tax, total, settled, anchor) in
        raw
    {
        let Ok(anchor_date) = NaiveDate::parse_from_str(&anchor, "%Y-%m-%d") else {
            continue; // 日期脏数据跳过（与借款台账口径一致）
        };
        let outstanding = round2((total - settled).max(0.0));
        let status = if outstanding <= AMOUNT_TOLERANCE {
            "已核清"
        } else if settled > AMOUNT_TOLERANCE {
            "部分核销"
        } else {
            "未核销"
        };
        let aging_days = (today - anchor_date).num_days().max(0);
        rows.push(ArLedgerRow {
            invoice_id: id,
            invoice_code: code,
            invoice_number: number,
            issue_date: issue,
            belong_month: month,
            buyer_name: buyer,
            partner_id: pid,
            partner_name: pname,
            amount,
            tax_amount: tax,
            total_amount: total,
            settled_amount: round2(settled),
            outstanding_amount: outstanding,
            status: status.into(),
            aging_days,
            aging_bucket: aging_bucket(aging_days).into(),
        });
    }
    Ok(rows)
}

/// 账龄汇总：客户 × 分桶矩阵（未核销余额），供导出。
pub fn ar_aging_summary(rows: &[ArLedgerRow]) -> Vec<ArAgingSummaryRow> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, ArAgingSummaryRow> = BTreeMap::new();
    for r in rows {
        if r.outstanding_amount <= AMOUNT_TOLERANCE {
            continue;
        }
        let key = r
            .partner_name
            .clone()
            .or_else(|| r.buyer_name.clone())
            .unwrap_or_else(|| "未知客户".into());
        let entry = map.entry(key.clone()).or_insert(ArAgingSummaryRow {
            partner_key: key,
            bucket_0_30: 0.0,
            bucket_31_60: 0.0,
            bucket_61_90: 0.0,
            bucket_91_180: 0.0,
            bucket_180_plus: 0.0,
            total_outstanding: 0.0,
        });
        let v = r.outstanding_amount;
        match r.aging_bucket.as_str() {
            "0-30" => entry.bucket_0_30 += v,
            "31-60" => entry.bucket_31_60 += v,
            "61-90" => entry.bucket_61_90 += v,
            "91-180" => entry.bucket_91_180 += v,
            _ => entry.bucket_180_plus += v,
        }
        entry.total_outstanding += v;
    }
    map.into_values().collect()
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::InvoiceInput;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::seed_gl_accounts(&conn).unwrap();
        crate::db::insert_default_data(&conn).unwrap();
        conn
    }

    fn insert_invoice(
        conn: &Connection,
        direction: &str,
        number: &str,
        total: f64,
        issue: &str,
        month: &str,
    ) -> i64 {
        crate::db::insert_invoice(
            conn,
            &InvoiceInput {
                invoice_code: None,
                invoice_number: Some(number.into()),
                invoice_type: Some("全电发票".into()),
                issue_date: Some(issue.into()),
                check_code: None,
                amount: Some((total / 1.13 * 100.0).round() / 100.0),
                tax_amount: Some(
                    ((total - (total / 1.13 * 100.0).round() / 100.0) * 100.0).round() / 100.0,
                ),
                total_amount: Some(total),
                seller_name: Some("示例科技公司".into()),
                seller_tax_id: None,
                buyer_name: Some("北京示例客户有限公司".into()),
                buyer_tax_id: None,
                expense_type_code: None,
                employee_id: None,
                belong_month: Some(month.into()),
                direction: Some(direction.into()),
                partner_id: None,
                remark: None,
                image_path: None,
                raw_ocr_json: None,
            },
            "",
            0,
        )
        .unwrap()
        .id
    }

    fn insert_settled_receipt(conn: &Connection, month: &str, amount: f64) -> i64 {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO fund_documents
             (document_no, document_type, belong_month, document_date, amount, summary, status,
              settled_at, created_at, updated_at)
             VALUES ('SK0001', 'receipt', ?1, ?1 || '-05', ?2, '收到货款', 'settled', ?3, ?3, ?3)",
            params![month, amount, now],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn sales_invoice_generates_revenue_voucher() {
        let conn = setup();
        let id = insert_invoice(
            &conn,
            "output",
            "XS00000001",
            11300.0,
            "2026-10-01",
            "2026-10",
        );
        let voucher = conn
            .query_row(
                "SELECT total_amount FROM vouchers WHERE source_type='sales_invoice' AND source_id=?1 AND status='active'",
                params![id],
                |r| r.get::<_, f64>(0),
            )
            .unwrap();
        assert!(
            (voucher - 11300.0).abs() < 0.01,
            "销项凭证金额应=价税合计: {voucher}"
        );
        // 分录：借 1122 全额 / 贷 6001 不含税 / 贷 2221 税额
        let lines: Vec<(String, f64, f64)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT account_code, debit_amount, credit_amount FROM voucher_lines \
                     WHERE voucher_id = (SELECT id FROM vouchers WHERE source_type='sales_invoice' AND source_id=?1)",
                )
                .unwrap();
            stmt.query_map(params![id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(lines.len(), 3);
        assert!(lines
            .iter()
            .any(|(c, d, _)| c == "1122" && (*d - 11300.0).abs() < 0.01));
        assert!(lines
            .iter()
            .any(|(c, _, cr)| c == "6001" && (cr - 10000.0).abs() < 0.01));
        assert!(lines
            .iter()
            .any(|(c, _, cr)| c == "2221" && (cr - 1300.0).abs() < 0.01));
    }

    #[test]
    fn input_invoice_still_expense_voucher_and_output_not() {
        let conn = setup();
        let in_id = insert_invoice(
            &conn,
            "input",
            "JX00000001",
            1130.0,
            "2026-10-01",
            "2026-10",
        );
        // 进项未挂报销 → 费用凭证（invoice_expense）
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vouchers WHERE source_type='invoice_expense' AND source_id=?1",
                params![in_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "进项发票仍走费用凭证");
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vouchers WHERE source_type='sales_invoice' AND source_id=?1",
                params![in_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn void_sales_invoice_reverses_voucher() {
        let conn = setup();
        let id = insert_invoice(
            &conn,
            "output",
            "XS00000002",
            5650.0,
            "2026-10-02",
            "2026-10",
        );
        crate::db::soft_delete_invoice(&conn, id).unwrap();
        let active: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vouchers WHERE source_type='sales_invoice' AND source_id=?1 AND status='active'",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(active, 0, "作废销项应联动作废收入凭证");
    }

    #[test]
    fn reimbursement_rejects_output_invoice() {
        let conn = setup();
        conn.execute(
            "INSERT INTO employees (employee_no, name, status, created_at, updated_at)
             VALUES ('E001', '员工甲', 'active', '2026-01-01', '2026-01-01')",
            [],
        )
        .unwrap();
        let emp_id = conn.last_insert_rowid();
        let out_id = insert_invoice(
            &conn,
            "output",
            "XS00000003",
            1000.0,
            "2026-10-03",
            "2026-10",
        );
        conn.execute(
            "UPDATE invoices SET employee_id = ?1 WHERE id = ?2",
            params![emp_id, out_id],
        )
        .unwrap();
        let err = crate::db::save_reimbursement_claim(
            &conn,
            &crate::models::ReimbursementClaimInput {
                id: None,
                employee_id: Some(emp_id),
                belong_month: "2026-10".into(),
                title: "测试报销".into(),
                invoice_ids: vec![out_id],
                remark: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("销项发票"), "{err}");
    }

    #[test]
    fn input_tax_ledger_excludes_output() {
        let conn = setup();
        insert_invoice(
            &conn,
            "input",
            "JX00000002",
            1130.0,
            "2026-10-04",
            "2026-10",
        );
        insert_invoice(
            &conn,
            "output",
            "XS00000004",
            2260.0,
            "2026-10-05",
            "2026-10",
        );
        let report = crate::db::get_input_tax_ledger(&conn, "2026-10", "2026-10").unwrap();
        assert_eq!(report.rows.len(), 1, "进项台账只含进项票");
        assert!(report.rows[0]
            .invoice_number
            .as_deref()
            .unwrap()
            .starts_with("jx"));
    }

    #[test]
    fn query_invoices_direction_filter() {
        let conn = setup();
        insert_invoice(&conn, "input", "JX00000003", 100.0, "2026-10-06", "2026-10");
        insert_invoice(
            &conn,
            "output",
            "XS00000005",
            200.0,
            "2026-10-07",
            "2026-10",
        );
        let all =
            crate::db::query_invoices(&conn, &crate::models::InvoiceQuery::default()).unwrap();
        assert_eq!(all.len(), 2, "无过滤返回全部（兼容既有调用）");
        let only_out = crate::db::query_invoices(
            &conn,
            &crate::models::InvoiceQuery {
                direction: Some("output".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(only_out.len(), 1);
        assert_eq!(only_out[0].direction.as_deref(), Some("output"));
    }

    #[test]
    fn allocation_flow_and_guards() {
        let conn = setup();
        let current = CurrentOperatorState::new();
        let inv = insert_invoice(
            &conn,
            "output",
            "XS00000006",
            11300.0,
            "2026-08-01",
            "2026-08",
        );
        let receipt = insert_settled_receipt(&conn, "2026-10", 8000.0);

        // 部分核销 8000
        let alloc = create_ar_allocation(
            &conn,
            &current,
            &ArAllocationInput {
                receipt_id: receipt,
                invoice_id: inv,
                amount: 8000.0,
                remark: Some("首次回款".into()),
            },
        )
        .unwrap();
        assert_eq!(alloc.status, "active");

        // 超余额拒绝（余 3300）
        let err = create_ar_allocation(
            &conn,
            &current,
            &ArAllocationInput {
                receipt_id: receipt,
                invoice_id: inv,
                amount: 5000.0,
                remark: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("超过"), "{err}");

        // 台账：部分核销 + 账龄
        let rows = get_ar_ledger(
            &conn,
            &ArLedgerQuery {
                as_of_date: Some("2026-10-15".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "部分核销");
        assert!((rows[0].settled_amount - 8000.0).abs() < 0.01);
        assert!((rows[0].outstanding_amount - 3300.0).abs() < 0.01);
        assert_eq!(rows[0].aging_days, 75); // 8-01 → 10-15
        assert_eq!(rows[0].aging_bucket, "61-90");

        // 进项发票不可核销
        let in_inv = insert_invoice(&conn, "input", "JX00000004", 100.0, "2026-10-08", "2026-10");
        let err = create_ar_allocation(
            &conn,
            &current,
            &ArAllocationInput {
                receipt_id: receipt,
                invoice_id: in_inv,
                amount: 50.0,
                remark: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("进项"), "{err}");

        // 取消核销：原因必填 + 恢复余额
        let err = cancel_ar_allocation(
            &conn,
            &current,
            &ArAllocationCancelInput {
                id: alloc.id,
                reason: "  ".into(),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("原因必填"));
        cancel_ar_allocation(
            &conn,
            &current,
            &ArAllocationCancelInput {
                id: alloc.id,
                reason: "回款单金额录错".into(),
            },
        )
        .unwrap();
        let rows = get_ar_ledger(&conn, &ArLedgerQuery::default()).unwrap();
        assert_eq!(rows[0].status, "未核销");
        assert!((rows[0].outstanding_amount - 11300.0).abs() < 0.01);
        // 重复取消拒绝
        let err = cancel_ar_allocation(
            &conn,
            &current,
            &ArAllocationCancelInput {
                id: alloc.id,
                reason: "再取消".into(),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("仅 active"));
    }

    #[test]
    fn month_close_blocks_allocation_and_cancel() {
        let conn = setup();
        let current = CurrentOperatorState::new();
        let inv = insert_invoice(
            &conn,
            "output",
            "XS00000007",
            1000.0,
            "2026-08-01",
            "2026-08",
        );
        let receipt = insert_settled_receipt(&conn, "2026-08", 1000.0);
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO month_closes (month, status, closed_at, closed_by, created_at, updated_at)
             VALUES ('2026-08', 'closed', ?1, '测试', ?1, ?1)",
            params![now],
        )
        .unwrap();
        // 双方任一月结均拦截
        let err = create_ar_allocation(
            &conn,
            &current,
            &ArAllocationInput {
                receipt_id: receipt,
                invoice_id: inv,
                amount: 500.0,
                remark: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("月结"), "{err}");
    }

    #[test]
    fn ar_overdue_dashboard_reminder() {
        let conn = setup();
        // 120 天前开票、未核销 → 提醒；30 天前的 → 不提醒
        insert_invoice(
            &conn,
            "output",
            "XS00000008",
            1000.0,
            "2026-06-15",
            "2026-06",
        );
        insert_invoice(
            &conn,
            "output",
            "XS00000009",
            2000.0,
            "2026-09-15",
            "2026-09",
        );
        let today = chrono::Local::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        // 造数日期相对今天不稳定 → 直接用固定基准日造世界（2026-10-15 基准：6-15=122 天，9-15=30 天）
        let items = crate::db::get_dashboard_reminders(&conn, "2026-10-15").unwrap();
        let ar: Vec<_> = items
            .iter()
            .filter(|i| i.category == "ar_overdue")
            .collect();
        assert_eq!(ar.len(), 1, "仅账龄>90 天的销项提醒: {items:?}");
        assert!(ar[0].title.contains("xs00000008"));
        assert!(ar[0].title.contains("账龄 122 天"));
        assert!((ar[0].amount.unwrap() - 1000.0).abs() < 0.01);
    }

    #[test]
    fn aging_summary_buckets() {
        let conn = setup();
        insert_invoice(&conn, "output", "XS00010", 100.0, "2026-10-01", "2026-10");
        insert_invoice(&conn, "output", "XS00020", 200.0, "2026-08-10", "2026-08");
        insert_invoice(&conn, "output", "XS00150", 300.0, "2026-04-01", "2026-04");
        let rows = get_ar_ledger(
            &conn,
            &ArLedgerQuery {
                as_of_date: Some("2026-10-15".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let summary = ar_aging_summary(&rows);
        assert_eq!(summary.len(), 1); // 同一客户聚合
        let row = &summary[0];
        assert!((row.bucket_0_30 - 100.0).abs() < 0.01);
        assert!((row.bucket_61_90 - 200.0).abs() < 0.01);
        assert!((row.bucket_180_plus - 300.0).abs() < 0.01);
        assert!((row.total_outstanding - 600.0).abs() < 0.01);
    }
}
