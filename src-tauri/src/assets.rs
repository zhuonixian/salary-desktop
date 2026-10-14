//! 第十三阶段：固定资产——登记、月度直线折旧、处置与台账（spec 2026-10-14 第 3 节）
//!
//! 凭证口径（小企业准则）：
//! - 购置：借 1601 固定资产（原值）/ 贷 counter_account（资金科目须带辅助，或应付类）
//! - 折旧：按月聚合一张凭证——借按资产费用科目分组（6602/5101/6601）/ 贷 1602 累计折旧
//! - 处置：借 1602 累计已提 / 借或贷 6711|6301 净损益 / 贷 1601 原值；处置收入加收款资金行
//! 月折旧 = 原值 × (1 − 残值率) ÷ 年限月数（2 位），末月尾差补齐；重复执行幂等。

use chrono::Utc;
use rusqlite::{params, Connection};

use crate::accounting::insert_voucher;
use crate::cashier::CurrentOperatorState;
use crate::errors::{AppError, AppResult};
use crate::models::{
    FixedAssetInput, FixedAssetLedgerRow, FixedAssetRecord, FixedDepreciationRunSummary,
};
use crate::models::{VoucherDraft, VoucherLineDraft};

const TOLERANCE: f64 = 0.005;

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn asset_no(purchase_date: &str) -> String {
    let date_compact = purchase_date.replace('-', "");
    let nanos = Utc::now()
        .timestamp_nanos_opt()
        .unwrap_or_else(|| Utc::now().timestamp_millis() as i64);
    format!("ZC{date_compact}{:06}", (nanos % 1_000_000) as u64)
}

const ASSET_CATEGORIES: &[&str] = &["building", "machinery", "vehicle", "electronics", "other"];

fn category_label(code: &str) -> &'static str {
    match code {
        "building" => "房屋建筑物",
        "machinery" => "机器设备",
        "vehicle" => "运输工具",
        "electronics" => "电子设备",
        _ => "其他",
    }
}

pub fn asset_category_label(code: &str) -> &'static str {
    category_label(code)
}

/// 月折旧额（2 位）；返回 (常规月折旧, 应提总月数)。
/// 应提总额 = 原值 × (1 − 残值率)，末月补尾差由调用方处理。
pub fn monthly_depreciation(asset: &FixedAssetRecord) -> (f64, i64) {
    let depreciable = asset.original_value * (1.0 - asset.salvage_rate);
    let months = asset.useful_life_months;
    (round2(depreciable / months as f64), months)
}

/// 累计已提（fixed_asset_depreciations 行合计）。
pub fn accumulated_depreciation(conn: &Connection, asset_id: i64) -> AppResult<f64> {
    let v: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM fixed_asset_depreciations WHERE asset_id = ?1",
        params![asset_id],
        |r| r.get(0),
    )?;
    Ok(round2(v))
}

/// 资产登记（购置凭证可选生成：counter_account_code 给定时）。
/// 贷方为资金科目（1001/1002/1012）时必须带 fund_account_id 辅助。
pub fn save_fixed_asset(
    conn: &Connection,
    current: &CurrentOperatorState,
    input: &FixedAssetInput,
) -> AppResult<FixedAssetRecord> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::InvalidParam("资产名称必填".into()));
    }
    if input.original_value <= 0.0 {
        return Err(AppError::InvalidParam("资产原值必须大于 0".into()));
    }
    if input.useful_life_months <= 0 {
        return Err(AppError::InvalidParam("折旧年限（月）必须大于 0".into()));
    }
    if !(0.0..=0.5).contains(&input.salvage_rate) {
        return Err(AppError::InvalidParam(
            "残值率应在 0% ~ 50% 之间（小数输入，如 0.05）".into(),
        ));
    }
    if !ASSET_CATEGORIES.contains(&input.category.as_str()) {
        return Err(AppError::InvalidParam(format!(
            "资产类别非法：{}（可选：{}）",
            input.category,
            ASSET_CATEGORIES.join("/")
        )));
    }
    if input.purchase_date.trim().is_empty() {
        return Err(AppError::InvalidParam("购置日期必填".into()));
    }
    let belong_month = input
        .belong_month
        .trim()
        .is_empty()
        .then(|| input.purchase_date[..7].to_string())
        .unwrap_or_else(|| input.belong_month.trim().to_string());
    crate::db::ensure_month_open(conn, &belong_month)?;
    // 费用科目存在性
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM gl_accounts WHERE code = ?1",
        params![input.expense_account_code],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Err(AppError::InvalidParam(format!(
            "折旧费用科目 {} 不存在",
            input.expense_account_code
        )));
    }

    let now = Utc::now().to_rfc3339();
    let (id, asset_no_val) = if let Some(id) = input.id {
        let existing = get_fixed_asset(conn, id)?;
        crate::db::ensure_month_open(conn, &existing.belong_month)?;
        // 已有折旧记录后不允许改原值/年限/残值率/购置月（口径会漂移）
        let dep_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM fixed_asset_depreciations WHERE asset_id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        if dep_count > 0
            && ((existing.original_value - input.original_value).abs() > TOLERANCE
                || existing.useful_life_months != input.useful_life_months
                || (existing.salvage_rate - input.salvage_rate).abs() > 1e-9
                || existing.belong_month != belong_month)
        {
            return Err(AppError::InvalidParam(
                "资产已计提折旧，原值/年限/残值率/归属月不可修改（如需调整请先处置后重新登记）"
                    .into(),
            ));
        }
        conn.execute(
            "UPDATE fixed_assets SET name=?1, category=?2, purchase_date=?3, belong_month=?4,
             original_value=?5, salvage_rate=?6, useful_life_months=?7, expense_account_code=?8,
             remark=?9, updated_at=?10 WHERE id=?11",
            params![
                name,
                input.category,
                input.purchase_date,
                belong_month,
                input.original_value,
                input.salvage_rate,
                input.useful_life_months,
                input.expense_account_code,
                input.remark,
                now,
                id
            ],
        )?;
        (id, existing.asset_no)
    } else {
        let no = asset_no(&input.purchase_date);
        conn.execute(
            "INSERT INTO fixed_assets
                (asset_no, name, category, purchase_date, belong_month, original_value,
                 salvage_rate, useful_life_months, expense_account_code, status, remark,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'in_use', ?10, ?11, ?11)",
            params![
                no,
                name,
                input.category,
                input.purchase_date,
                belong_month,
                input.original_value,
                input.salvage_rate,
                input.useful_life_months,
                input.expense_account_code,
                input.remark,
                now
            ],
        )?;
        (conn.last_insert_rowid(), no)
    };

    // 购置凭证（可选）：借 1601 / 贷 counter_account
    if let Some(counter) = input
        .counter_account_code
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let counter = counter.to_string();
        let fund_codes = crate::db::STAGE7_FUND_GL_CODES;
        if fund_codes.contains(&counter.as_str()) && input.counter_fund_account_id.is_none() {
            return Err(AppError::InvalidParam(format!(
                "购置贷方为资金科目 {counter} 时必须选择资金账户（辅助核算）"
            )));
        }
        // 幂等：已有 active 购置凭证则跳过
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM vouchers WHERE source_type='asset_purchase' AND source_id=?1 AND status='active'",
            params![id],
            |r| r.get(0),
        )?;
        if exists == 0 {
            insert_voucher(
                conn,
                &VoucherDraft {
                    belong_month: belong_month.clone(),
                    voucher_date: input.purchase_date.clone(),
                    source_type: "asset_purchase".into(),
                    source_id: id,
                    remark: Some(format!("购置固定资产 {name}")),
                    lines: vec![
                        VoucherLineDraft {
                            account_code: "1601".into(),
                            debit_amount: input.original_value,
                            credit_amount: 0.0,
                            summary: Some(format!("购置 {name}")),
                            fund_account_id: None,
                        },
                        VoucherLineDraft {
                            account_code: counter,
                            debit_amount: 0.0,
                            credit_amount: input.original_value,
                            summary: Some(format!("购置 {name}（贷方）")),
                            fund_account_id: input.counter_fund_account_id,
                        },
                    ],
                },
            )?;
        }
    }

    crate::db::log_operation(
        conn,
        "save_fixed_asset",
        &format!(
            "{}固定资产 {}（{}），原值 {:.2}，年限 {} 月",
            if input.id.is_some() {
                "更新"
            } else {
                "登记"
            },
            name,
            asset_no_val,
            input.original_value,
            input.useful_life_months
        ),
        &crate::cashier::current_operator_name(conn, current),
        Some(&format!("asset_id={id}")),
    )?;
    get_fixed_asset(conn, id)
}

fn row_to_asset(row: &rusqlite::Row<'_>) -> rusqlite::Result<FixedAssetRecord> {
    Ok(FixedAssetRecord {
        id: row.get(0)?,
        asset_no: row.get(1)?,
        name: row.get(2)?,
        category: row.get(3)?,
        category_label: category_label(&row.get::<_, String>(3)?).to_string(),
        purchase_date: row.get(4)?,
        belong_month: row.get(5)?,
        original_value: row.get(6)?,
        salvage_rate: row.get(7)?,
        useful_life_months: row.get(8)?,
        expense_account_code: row.get(9)?,
        status: row.get(10)?,
        disposal_date: row.get(11)?,
        disposal_income: row.get(12)?,
        remark: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

const ASSET_SELECT: &str = "id, asset_no, name, category, purchase_date, belong_month, \
     original_value, salvage_rate, useful_life_months, expense_account_code, status, \
     disposal_date, disposal_income, remark, created_at, updated_at";

pub fn get_fixed_asset(conn: &Connection, id: i64) -> AppResult<FixedAssetRecord> {
    conn.query_row(
        &format!("SELECT {ASSET_SELECT} FROM fixed_assets WHERE id = ?1"),
        params![id],
        row_to_asset,
    )
    .map_err(|_| AppError::NotFound(format!("资产ID={id}未找到")))
}

pub fn get_fixed_assets(
    conn: &Connection,
    keyword: Option<&str>,
    status: Option<&str>,
) -> AppResult<Vec<FixedAssetRecord>> {
    let mut sql = format!("SELECT {ASSET_SELECT} FROM fixed_assets");
    let mut conds = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(kw) = keyword.map(str::trim).filter(|s| !s.is_empty()) {
        conds.push("(name LIKE ?1 OR asset_no LIKE ?1 OR remark LIKE ?1)".to_string());
        params_vec.push(Box::new(format!("%{kw}%")));
    }
    if let Some(st) = status.map(str::trim).filter(|s| !s.is_empty()) {
        let idx = if conds.is_empty() { 1 } else { 2 };
        conds.push(format!("status = ?{idx}"));
        params_vec.push(Box::new(st.to_string()));
    }
    if !conds.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conds.join(" AND "));
    }
    sql.push_str(" ORDER BY purchase_date, id");
    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(refs.as_slice(), row_to_asset)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 目标月应折旧的资产：in_use、购置月 < 目标月、已提月数 < 年限。
fn depreciable_assets(conn: &Connection, month: &str) -> AppResult<Vec<FixedAssetRecord>> {
    let assets = get_fixed_assets(conn, None, Some("in_use"))?;
    let mut out = Vec::new();
    for a in assets {
        if a.belong_month.as_str() >= month {
            continue; // 购置当月不提（次月起提，小企业准则惯例）
        }
        let dep_months: i64 = conn.query_row(
            "SELECT COUNT(*) FROM fixed_asset_depreciations WHERE asset_id = ?1",
            params![a.id],
            |r| r.get(0),
        )?;
        if dep_months < a.useful_life_months {
            out.push(a);
        }
    }
    Ok(out)
}

/// 月度折旧批处理：每资产一行记录、按月聚合一张凭证（借按费用科目分组/贷 1602）。
/// 幂等：已有当月记录的资产跳过。返回 (计提资产数, 凭证号)。
pub fn run_asset_depreciation(
    conn: &Connection,
    month: &str,
    current: &CurrentOperatorState,
) -> AppResult<FixedDepreciationRunSummary> {
    crate::db::validate_input_tax_ledger_month(month, "折旧月份")?;
    crate::db::ensure_month_open(conn, month)?;

    let assets = depreciable_assets(conn, month)?;
    let mut pending = Vec::new();
    for a in &assets {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM fixed_asset_depreciations WHERE asset_id = ?1 AND month = ?2",
            params![a.id, month],
            |r| r.get(0),
        )?;
        if exists == 0 {
            pending.push(a);
        }
    }
    if pending.is_empty() {
        return Ok(FixedDepreciationRunSummary {
            month: month.to_string(),
            asset_count: 0,
            total_amount: 0.0,
            voucher_no: None,
        });
    }

    // 计算金额：末月补尾差（应提总额 − 已提 − 常规×剩余月数）
    let mut rows: Vec<(i64, f64, String, String)> = Vec::new(); // (asset_id, amount, expense_code, name)
    for a in &pending {
        let (regular, _total_months) = monthly_depreciation(a);
        let already = accumulated_depreciation(conn, a.id)?;
        let depreciable = round2(a.original_value * (1.0 - a.salvage_rate));
        let dep_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM fixed_asset_depreciations WHERE asset_id = ?1",
            params![a.id],
            |r| r.get(0),
        )?;
        let remaining_months = a.useful_life_months - dep_count;
        let amount = if remaining_months <= 1 {
            round2(depreciable - already)
        } else {
            regular
        };
        if amount <= TOLERANCE {
            continue; // 残值率 100% 等边界：无可提额
        }
        rows.push((a.id, amount, a.expense_account_code.clone(), a.name.clone()));
    }
    if rows.is_empty() {
        return Ok(FixedDepreciationRunSummary {
            month: month.to_string(),
            asset_count: 0,
            total_amount: 0.0,
            voucher_no: None,
        });
    }

    // 凭证：借按费用科目分组 / 贷 1602 合计
    let mut expense_groups: Vec<(String, f64)> = Vec::new();
    for (_, amount, code, _) in &rows {
        if let Some(e) = expense_groups.iter_mut().find(|(c, _)| c == code) {
            e.1 += amount;
        } else {
            expense_groups.push((code.clone(), *amount));
        }
    }
    let total: f64 = rows.iter().map(|(_, a, _, _)| a).sum();
    let mut lines: Vec<VoucherLineDraft> = expense_groups
        .into_iter()
        .map(|(code, amt)| VoucherLineDraft {
            account_code: code,
            debit_amount: round2(amt),
            credit_amount: 0.0,
            summary: Some(format!("{month} 固定资产折旧")),
            fund_account_id: None,
        })
        .collect();
    lines.push(VoucherLineDraft {
        account_code: "1602".into(),
        debit_amount: 0.0,
        credit_amount: round2(total),
        summary: Some(format!("{month} 累计折旧")),
        fund_account_id: None,
    });

    let now = Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;
    // source_id 用稳定值（月字符串 hash 不可行）——凭证与记录同一事务：先插凭证拿 id，
    // 再写折旧行引用。source_type='asset_depreciation'，source_id 用凭证自身 id 不可能
    // （先有鸡）；改用「月」的数字形式 yyyymm 作 source_id，保证按月唯一可追溯。
    let source_id: i64 = month.replace('-', "").parse().unwrap_or(0);
    let voucher = insert_voucher(
        &tx,
        &VoucherDraft {
            belong_month: month.to_string(),
            voucher_date: format!("{month}-28"),
            source_type: "asset_depreciation".into(),
            source_id,
            remark: Some(format!("{month} 固定资产折旧计提")),
            lines,
        },
    )?;
    for (asset_id, amount, _, _) in &rows {
        tx.execute(
            "INSERT INTO fixed_asset_depreciations (asset_id, month, amount, voucher_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![asset_id, month, amount, voucher.id, now],
        )?;
    }
    tx.commit()?;

    crate::db::log_operation(
        conn,
        "run_asset_depreciation",
        &format!(
            "{month} 计提折旧：{} 项资产，合计 {:.2}，凭证 {}",
            rows.len(),
            total,
            voucher.voucher_no
        ),
        &crate::cashier::current_operator_name(conn, current),
        None,
    )?;
    Ok(FixedDepreciationRunSummary {
        month: month.to_string(),
        asset_count: rows.len(),
        total_amount: round2(total),
        voucher_no: Some(voucher.voucher_no),
    })
}

/// 处置：状态 → disposed + 清理凭证。
/// 借 1602 累计已提 / 借或贷 净损益（6711 损失 / 6301 收益）/ 贷 1601 原值；
/// 处置收入 > 0 时另加资金收款行（须带辅助）。
pub fn dispose_fixed_asset(
    conn: &Connection,
    current: &CurrentOperatorState,
    id: i64,
    disposal_date: &str,
    disposal_income: f64,
    income_fund_account_id: Option<i64>,
    remark: Option<&str>,
) -> AppResult<FixedAssetRecord> {
    if disposal_date.trim().is_empty() {
        return Err(AppError::InvalidParam("处置日期必填".into()));
    }
    if disposal_income < 0.0 {
        return Err(AppError::InvalidParam("处置收入不能为负".into()));
    }
    let asset = get_fixed_asset(conn, id)?;
    if asset.status == "disposed" {
        return Err(AppError::InvalidParam(format!(
            "资产 {} 已于 {} 处置",
            asset.asset_no,
            asset.disposal_date.as_deref().unwrap_or("-")
        )));
    }
    let month = disposal_date[..7].to_string();
    crate::db::ensure_month_open(conn, &month)?;
    crate::db::ensure_month_open(conn, &asset.belong_month)?;
    let accumulated = accumulated_depreciation(conn, id)?;
    let net_book = round2(asset.original_value - accumulated);
    // 净损益 = 处置收入 − 净值
    let net_result = round2(disposal_income - net_book);

    let mut lines = vec![
        VoucherLineDraft {
            account_code: "1602".into(),
            debit_amount: accumulated,
            credit_amount: 0.0,
            summary: Some(format!("处置 {} 转出累计折旧", asset.name)),
            fund_account_id: None,
        },
        VoucherLineDraft {
            account_code: "1601".into(),
            debit_amount: 0.0,
            credit_amount: asset.original_value,
            summary: Some(format!("处置 {} 转出原值", asset.name)),
            fund_account_id: None,
        },
    ];
    if disposal_income > 0.0 {
        if income_fund_account_id.is_none() {
            return Err(AppError::InvalidParam(
                "有处置收入时必须选择收款资金账户（辅助核算）".into(),
            ));
        }
        lines.push(VoucherLineDraft {
            account_code: "1002".into(),
            debit_amount: disposal_income,
            credit_amount: 0.0,
            summary: Some(format!("处置 {} 收到款项", asset.name)),
            fund_account_id: income_fund_account_id,
        });
    }
    if net_result.abs() > TOLERANCE {
        if net_result < 0.0 {
            lines.push(VoucherLineDraft {
                account_code: "6711".into(),
                debit_amount: -net_result,
                credit_amount: 0.0,
                summary: Some(format!("处置 {} 净损失", asset.name)),
                fund_account_id: None,
            });
        } else {
            lines.push(VoucherLineDraft {
                account_code: "6301".into(),
                debit_amount: 0.0,
                credit_amount: net_result,
                summary: Some(format!("处置 {} 净收益", asset.name)),
                fund_account_id: None,
            });
        }
    }

    let now = Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;
    let voucher = insert_voucher(
        &tx,
        &VoucherDraft {
            belong_month: month.clone(),
            voucher_date: disposal_date.to_string(),
            source_type: "asset_disposal".into(),
            source_id: id,
            remark: Some(format!(
                "处置固定资产 {}{}",
                asset.name,
                remark
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|r| format!("：{r}"))
                    .unwrap_or_default()
            )),
            lines,
        },
    )?;
    tx.execute(
        "UPDATE fixed_assets SET status='disposed', disposal_date=?1, disposal_income=?2, updated_at=?3 WHERE id=?4",
        params![disposal_date, disposal_income, now, id],
    )?;
    tx.commit()?;

    crate::db::log_operation(
        conn,
        "dispose_fixed_asset",
        &format!(
            "处置固定资产 {}（{}）：原值 {:.2}，累计折旧 {:.2}，收入 {:.2}，净{} {:.2}，凭证 {}",
            asset.name,
            asset.asset_no,
            asset.original_value,
            accumulated,
            disposal_income,
            if net_result < 0.0 { "损失" } else { "收益" },
            net_result.abs(),
            voucher.voucher_no
        ),
        &crate::cashier::current_operator_name(conn, current),
        Some(&format!("asset_id={id}")),
    )?;
    get_fixed_asset(conn, id)
}

/// 资产台账：原值/累计已提/净值/月折旧/剩余月数。
pub fn get_fixed_asset_ledger(
    conn: &Connection,
    keyword: Option<&str>,
    status: Option<&str>,
) -> AppResult<Vec<FixedAssetLedgerRow>> {
    let assets = get_fixed_assets(conn, keyword, status)?;
    let mut rows = Vec::with_capacity(assets.len());
    for a in assets {
        let accumulated = accumulated_depreciation(conn, a.id)?;
        let (regular, _) = monthly_depreciation(&a);
        let dep_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM fixed_asset_depreciations WHERE asset_id = ?1",
            params![a.id],
            |r| r.get(0),
        )?;
        let remaining = (a.useful_life_months - dep_count).max(0);
        rows.push(FixedAssetLedgerRow {
            asset_id: a.id,
            asset_no: a.asset_no.clone(),
            name: a.name.clone(),
            category_label: a.category_label.clone(),
            purchase_date: a.purchase_date.clone(),
            belong_month: a.belong_month.clone(),
            original_value: a.original_value,
            salvage_rate: a.salvage_rate,
            useful_life_months: a.useful_life_months,
            expense_account_code: a.expense_account_code.clone(),
            status: a.status.clone(),
            disposal_date: a.disposal_date.clone(),
            disposal_income: a.disposal_income,
            accumulated_depreciation: accumulated,
            net_book_value: round2(a.original_value - accumulated),
            monthly_amount: if a.status == "in_use" && remaining > 0 {
                regular
            } else {
                0.0
            },
            depreciated_months: dep_count,
            remaining_months: remaining,
        });
    }
    Ok(rows)
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::FixedAssetInput;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::seed_gl_accounts(&conn).unwrap();
        crate::db::insert_default_data(&conn).unwrap();
        conn
    }

    fn asset_input(name: &str, value: f64) -> FixedAssetInput {
        FixedAssetInput {
            id: None,
            name: name.into(),
            category: "electronics".into(),
            purchase_date: "2026-01-15".into(),
            belong_month: "2026-01".into(),
            original_value: value,
            salvage_rate: 0.05,
            useful_life_months: 36,
            expense_account_code: "6602".into(),
            counter_account_code: None,
            counter_fund_account_id: None,
            remark: None,
        }
    }

    #[test]
    fn manual_voucher_kinds_and_guards() {
        let conn = setup();
        // 转账凭证：借管理费用 / 贷其他应付款
        let v = crate::accounting::create_manual_voucher(
            &conn,
            "2026-10",
            "2026-10-10",
            "transfer_voucher",
            &[
                crate::accounting::ManualVoucherLineInput {
                    account_code: "6602".into(),
                    debit_amount: 500.0,
                    credit_amount: 0.0,
                    summary: Some("暂估费用".into()),
                    fund_account_id: None,
                },
                crate::accounting::ManualVoucherLineInput {
                    account_code: "2241".into(),
                    debit_amount: 0.0,
                    credit_amount: 500.0,
                    summary: None,
                    fund_account_id: None,
                },
            ],
            Some("月末暂估"),
        )
        .unwrap();
        assert!(v.voucher_no.starts_with("记-"));

        // 收款凭证缺资金科目 → 拒绝
        let err = crate::accounting::create_manual_voucher(
            &conn,
            "2026-10",
            "2026-10-11",
            "receipt_voucher",
            &[
                crate::accounting::ManualVoucherLineInput {
                    account_code: "1122".into(),
                    debit_amount: 100.0,
                    credit_amount: 0.0,
                    summary: None,
                    fund_account_id: None,
                },
                crate::accounting::ManualVoucherLineInput {
                    account_code: "6001".into(),
                    debit_amount: 0.0,
                    credit_amount: 100.0,
                    summary: None,
                    fund_account_id: None,
                },
            ],
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("资金科目"), "{err}");

        // 资金科目缺辅助 → 拒绝
        let err = crate::accounting::create_manual_voucher(
            &conn,
            "2026-10",
            "2026-10-12",
            "receipt_voucher",
            &[
                crate::accounting::ManualVoucherLineInput {
                    account_code: "1002".into(),
                    debit_amount: 100.0,
                    credit_amount: 0.0,
                    summary: None,
                    fund_account_id: None,
                },
                crate::accounting::ManualVoucherLineInput {
                    account_code: "1122".into(),
                    debit_amount: 0.0,
                    credit_amount: 100.0,
                    summary: None,
                    fund_account_id: None,
                },
            ],
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("资金账户"), "{err}");

        // 单行 → 拒绝
        let err = crate::accounting::create_manual_voucher(
            &conn,
            "2026-10",
            "2026-10-13",
            "transfer_voucher",
            &[crate::accounting::ManualVoucherLineInput {
                account_code: "6602".into(),
                debit_amount: 1.0,
                credit_amount: 0.0,
                summary: None,
                fund_account_id: None,
            }],
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("两行"));

        // 作废 + 月结拦截
        crate::accounting::void_manual_voucher(&conn, v.id).unwrap();
        let active: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vouchers WHERE id=?1 AND status='active'",
                params![v.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(active, 0);
    }

    #[test]
    fn asset_register_purchase_voucher_and_depreciation() {
        let conn = setup();
        let current = CurrentOperatorState::new();
        // 登记（贷方银行存款，带辅助）
        conn.execute(
            "INSERT INTO fund_accounts (account_code, name, account_type, gl_account_code, is_default, is_active, created_at, updated_at)
             VALUES ('B01', '测试银行户', 'bank', '1002', 1, 1, '2026-01-01', '2026-01-01')",
            [],
        )
        .unwrap();
        let fund_id = conn.last_insert_rowid();
        let mut input = asset_input("测试电脑", 12000.0);
        input.counter_account_code = Some("1002".into());
        input.counter_fund_account_id = Some(fund_id);
        let asset = save_fixed_asset(&conn, &current, &input).unwrap();
        assert!(asset.asset_no.starts_with("ZC202601"));

        // 购置凭证：借 1601 / 贷 1002（辅助）
        let (debit1601, credit1002): (f64, f64) = conn
            .query_row(
                "SELECT
                   (SELECT COALESCE(SUM(debit_amount),0) FROM voucher_lines l JOIN vouchers v ON v.id=l.voucher_id
                    WHERE v.source_type='asset_purchase' AND v.source_id=?1 AND l.account_code='1601'),
                   (SELECT COALESCE(SUM(credit_amount),0) FROM voucher_lines l JOIN vouchers v ON v.id=l.voucher_id
                    WHERE v.source_type='asset_purchase' AND v.source_id=?1 AND l.account_code='1002')",
                params![asset.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!((debit1601 - 12000.0).abs() < 0.01);
        assert!((credit1002 - 12000.0).abs() < 0.01);

        // 折旧：月折旧 = 12000×0.95/36 = 316.67
        let s1 = run_asset_depreciation(&conn, "2026-02", &current).unwrap();
        assert_eq!(s1.asset_count, 1);
        assert!(
            (s1.total_amount - 316.67).abs() < 0.01,
            "{}",
            s1.total_amount
        );
        // 幂等重跑
        let s2 = run_asset_depreciation(&conn, "2026-02", &current).unwrap();
        assert_eq!(s2.asset_count, 0);
        // 次月再提
        let s3 = run_asset_depreciation(&conn, "2026-03", &current).unwrap();
        assert_eq!(s3.asset_count, 1);
        // 购置当月（2026-01）不提
        let s0 = run_asset_depreciation(&conn, "2026-01", &current).unwrap();
        assert_eq!(s0.asset_count, 0);
        // 台账
        let ledger = get_fixed_asset_ledger(&conn, None, None).unwrap();
        assert!((ledger[0].accumulated_depreciation - 633.34).abs() < 0.01);
        assert!((ledger[0].net_book_value - 11366.66).abs() < 0.01);
        assert_eq!(ledger[0].remaining_months, 34);

        // 已提折旧后改原值 → 拒绝
        let mut bad = asset_input("测试电脑", 9999.0);
        bad.id = Some(asset.id);
        let err = save_fixed_asset(&conn, &current, &bad).unwrap_err();
        assert!(err.to_string().contains("已计提折旧"), "{err}");
    }

    #[test]
    fn depreciation_last_month_tail() {
        let conn = setup();
        let current = CurrentOperatorState::new();
        // 2 个月年限、无残值：原值 100.01 → 月 50，末月补 0.01
        let mut input = asset_input("短期资产", 100.01);
        input.useful_life_months = 2;
        input.salvage_rate = 0.0;
        input.belong_month = "2026-01".into();
        let asset = save_fixed_asset(&conn, &current, &input).unwrap();
        let s1 = run_asset_depreciation(&conn, "2026-02", &current).unwrap();
        let s2 = run_asset_depreciation(&conn, "2026-03", &current).unwrap();
        // 首月常规（50.00 或 50.01 视舍入），末月补齐尾差，两月合计恰为应提总额
        assert!(
            (s1.total_amount - 50.0).abs() < 0.011,
            "{}",
            s1.total_amount
        );
        assert!(
            ((s1.total_amount + s2.total_amount) - 100.01).abs() < 0.001,
            "{} + {}",
            s1.total_amount,
            s2.total_amount
        );
        // 提满后不再提
        let s3 = run_asset_depreciation(&conn, "2026-04", &current).unwrap();
        assert_eq!(s3.asset_count, 0);
        assert!((accumulated_depreciation(&conn, asset.id).unwrap() - 100.01).abs() < 0.001);
    }

    #[test]
    fn depreciation_multiple_assets_aggregate_voucher() {
        let conn = setup();
        let current = CurrentOperatorState::new();
        let mut a1 = asset_input("电脑", 3600.0);
        a1.useful_life_months = 36;
        a1.salvage_rate = 0.0; // 100/月
        a1.expense_account_code = "6602".into();
        let mut a2 = asset_input("机床", 7200.0);
        a2.category = "machinery".into();
        a2.useful_life_months = 36;
        a2.salvage_rate = 0.0; // 200/月
        a2.expense_account_code = "5101".into();
        save_fixed_asset(&conn, &current, &a1).unwrap();
        save_fixed_asset(&conn, &current, &a2).unwrap();

        let s = run_asset_depreciation(&conn, "2026-02", &current).unwrap();
        assert_eq!(s.asset_count, 2);
        assert!((s.total_amount - 300.0).abs() < 0.001);
        // 聚合凭证：6602 借 100、5101 借 200、1602 贷 300
        let lines: Vec<(String, f64, f64)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT l.account_code, l.debit_amount, l.credit_amount FROM voucher_lines l
                     JOIN vouchers v ON v.id = l.voucher_id
                     WHERE v.source_type='asset_depreciation' AND v.belong_month='2026-02'
                     ORDER BY l.account_code",
                )
                .unwrap();
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(lines.len(), 3);
        assert!(lines
            .iter()
            .any(|(c, d, _)| c == "1602" && (*d - 0.0).abs() < 0.001));
        assert!(lines
            .iter()
            .any(|(c, d, _)| c == "5101" && (*d - 200.0).abs() < 0.001));
        assert!(lines
            .iter()
            .any(|(c, d, _)| c == "6602" && (*d - 100.0).abs() < 0.001));
    }

    #[test]
    fn disposal_voucher_and_guards() {
        let conn = setup();
        let current = CurrentOperatorState::new();
        conn.execute(
            "INSERT INTO fund_accounts (account_code, name, account_type, gl_account_code, is_default, is_active, created_at, updated_at)
             VALUES ('B01', '测试银行户', 'bank', '1002', 1, 1, '2026-01-01', '2026-01-01')",
            [],
        )
        .unwrap();
        let fund_id = conn.last_insert_rowid();
        let mut input = asset_input("处置设备", 1000.0);
        input.useful_life_months = 10;
        input.salvage_rate = 0.0; // 100/月
        input.belong_month = "2026-01".into();
        let asset = save_fixed_asset(&conn, &current, &input).unwrap();
        run_asset_depreciation(&conn, "2026-02", &current).unwrap();
        run_asset_depreciation(&conn, "2026-03", &current).unwrap();
        // 累计 200，净值 800；处置收入 500 → 净损失 300
        let disposed = dispose_fixed_asset(
            &conn,
            &current,
            asset.id,
            "2026-04-10",
            500.0,
            Some(fund_id),
            None,
        )
        .unwrap();
        assert_eq!(disposed.status, "disposed");
        let lines: Vec<(String, f64, f64)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT l.account_code, l.debit_amount, l.credit_amount FROM voucher_lines l
                     JOIN vouchers v ON v.id = l.voucher_id
                     WHERE v.source_type='asset_disposal' AND v.source_id=?1",
                )
                .unwrap();
            stmt.query_map(params![asset.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        // 借1602=200 借1002=500 借6711=300 贷1601=1000
        assert!(lines
            .iter()
            .any(|(c, d, _)| c == "1602" && (*d - 200.0).abs() < 0.001));
        assert!(lines
            .iter()
            .any(|(c, d, _)| c == "1002" && (*d - 500.0).abs() < 0.001));
        assert!(lines
            .iter()
            .any(|(c, d, _)| c == "6711" && (*d - 300.0).abs() < 0.001));
        assert!(lines
            .iter()
            .any(|(c, _, cr)| c == "1601" && (*cr - 1000.0).abs() < 0.001));

        // 处置后折旧停止 + 重复处置拒绝
        let s = run_asset_depreciation(&conn, "2026-04", &current).unwrap();
        assert_eq!(s.asset_count, 0);
        let err = dispose_fixed_asset(&conn, &current, asset.id, "2026-05-01", 0.0, None, None)
            .unwrap_err();
        assert!(err.to_string().contains("已于"), "{err}");

        // 有处置收入但没选资金账户 → 拒绝
        let a2 = save_fixed_asset(&conn, &current, &asset_input("第二台", 500.0)).unwrap();
        let err = dispose_fixed_asset(&conn, &current, a2.id, "2026-04-11", 100.0, None, None)
            .unwrap_err();
        assert!(err.to_string().contains("收款资金账户"), "{err}");
    }

    #[test]
    fn month_close_blocks_asset_ops() {
        let conn = setup();
        let current = CurrentOperatorState::new();
        let asset = save_fixed_asset(&conn, &current, &asset_input("月结资产", 600.0)).unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO month_closes (month, status, closed_at, closed_by, created_at, updated_at)
             VALUES ('2026-01', 'closed', ?1, '测试', ?1, ?1)",
            params![now],
        )
        .unwrap();
        // 处置日期落在已月结月（2026-01）→ 拦截
        let e2 = dispose_fixed_asset(&conn, &current, asset.id, "2026-01-20", 0.0, None, None)
            .unwrap_err();
        assert!(e2.to_string().contains("月结"), "{e2}");
    }

    #[test]
    fn input_validation() {
        let conn = setup();
        let current = CurrentOperatorState::new();
        let mut bad = asset_input("零值", 0.0);
        let err = save_fixed_asset(&conn, &current, &bad).unwrap_err();
        assert!(err.to_string().contains("大于 0"));
        bad.original_value = 100.0;
        bad.salvage_rate = 0.9;
        let err = save_fixed_asset(&conn, &current, &bad).unwrap_err();
        assert!(err.to_string().contains("残值率"));
        bad.salvage_rate = 0.05;
        bad.category = "bogus".into();
        let err = save_fixed_asset(&conn, &current, &bad).unwrap_err();
        assert!(err.to_string().contains("资产类别"));
    }
}
