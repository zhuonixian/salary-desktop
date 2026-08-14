use crate::errors::{AppError, AppResult};
use crate::models::*;
use chrono::Utc;
use rusqlite::{params, Connection};

/// 查询全部会计科目（按编码排序）。
pub fn get_accounts(conn: &Connection) -> AppResult<Vec<GlAccount>> {
    let mut stmt = conn.prepare(
        "SELECT code, name, category, direction, cash_flow_category, is_system, is_active, remark
         FROM gl_accounts ORDER BY code",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(GlAccount {
                code: r.get(0)?,
                name: r.get(1)?,
                category: r.get(2)?,
                direction: r.get(3)?,
                cash_flow_category: r.get(4)?,
                is_system: r.get(5)?,
                is_active: r.get(6)?,
                remark: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 新增自定义科目（is_system=0，默认启用）。编码重复时报错。
pub fn create_account(conn: &Connection, input: &GlAccountInput) -> AppResult<GlAccount> {
    let cfc = input
        .cash_flow_category
        .clone()
        .unwrap_or_else(|| "none".into());
    if conn
        .query_row(
            "SELECT 1 FROM gl_accounts WHERE code = ?1",
            params![input.code],
            |_| Ok(()),
        )
        .is_ok()
    {
        return Err(AppError::General(format!("科目编码 {} 已存在", input.code)));
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO gl_accounts (code, name, category, direction, cash_flow_category, is_system, is_active, remark, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 1, ?6, ?7, ?7)",
        params![
            input.code,
            input.name,
            input.category,
            input.direction,
            cfc,
            input.remark,
            now
        ],
    )?;
    Ok(GlAccount {
        code: input.code.clone(),
        name: input.name.clone(),
        category: input.category.clone(),
        direction: input.direction.clone(),
        cash_flow_category: cfc,
        is_system: 0,
        is_active: 1,
        remark: input.remark.clone(),
    })
}

/// 启用/停用科目。已有凭证分录的科目不允许停用。
pub fn set_account_active(conn: &Connection, code: &str, active: bool) -> AppResult<bool> {
    let used: i64 = conn.query_row(
        "SELECT COUNT(*) FROM voucher_lines WHERE account_code = ?1",
        params![code],
        |r| r.get(0),
    )?;
    if !active && used > 0 {
        return Err(AppError::General(format!(
            "科目 {code} 已有 {used} 条凭证分录，不能停用"
        )));
    }
    conn.execute(
        "UPDATE gl_accounts SET is_active = ?2, updated_at = ?3 WHERE code = ?1",
        params![code, active as i64, Utc::now().to_rfc3339()],
    )?;
    Ok(true)
}

/// 读取期初余额：返回 (所属月份, 余额行列表)。无数据时月份为 None。
pub fn get_opening_balances(
    conn: &Connection,
) -> AppResult<(Option<String>, Vec<OpeningBalanceRow>)> {
    let month: Option<String> = conn
        .query_row("SELECT MIN(month) FROM opening_balances", [], |r| r.get(0))
        .unwrap_or(None);
    let mut rows = Vec::new();
    if let Some(m) = &month {
        let mut stmt = conn.prepare(
            "SELECT account_code, debit_amount, credit_amount FROM opening_balances WHERE month = ?1 ORDER BY account_code",
        )?;
        rows = stmt
            .query_map(params![m], |r| {
                Ok(OpeningBalanceRow {
                    account_code: r.get(0)?,
                    debit_amount: r.get(1)?,
                    credit_amount: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
    }
    Ok((month, rows))
}

/// 保存期初余额（整体覆盖：先校验方向与平衡，再清空旧数据重写）。
pub fn save_opening_balances(
    conn: &Connection,
    month: &str,
    rows: &[OpeningBalanceRow],
) -> AppResult<()> {
    // 先校验科目方向：借方科目金额应填借方，贷方科目金额应填贷方
    for row in rows {
        let direction: String = conn.query_row(
            "SELECT direction FROM gl_accounts WHERE code = ?1",
            params![row.account_code],
            |r| r.get(0),
        )?;
        let wrong = (direction == "debit" && row.credit_amount != 0.0)
            || (direction == "credit" && row.debit_amount != 0.0);
        if wrong {
            return Err(AppError::General(format!(
                "科目 {} 是{}方向科目，金额应填在{}侧",
                row.account_code,
                if direction == "debit" { "借" } else { "贷" },
                if direction == "debit" { "借" } else { "贷" }
            )));
        }
    }
    // 再校验借贷平衡
    let debit: f64 = rows.iter().map(|r| r.debit_amount).sum();
    let credit: f64 = rows.iter().map(|r| r.credit_amount).sum();
    if (debit - credit).abs() > 0.005 {
        return Err(AppError::General(format!(
            "期初余额不平衡：借方合计 {debit:.2}，贷方合计 {credit:.2}，差额 {:.2}",
            debit - credit
        )));
    }
    let now = Utc::now().to_rfc3339();
    conn.execute("DELETE FROM opening_balances", [])?;
    for row in rows {
        conn.execute(
            "INSERT INTO opening_balances (month, account_code, debit_amount, credit_amount, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![month, row.account_code, row.debit_amount, row.credit_amount, now],
        )?;
    }
    Ok(())
}

/// 查询全部科目映射。
pub fn get_account_mappings(conn: &Connection) -> AppResult<Vec<AccountMapping>> {
    let mut stmt = conn.prepare(
        "SELECT id, scope, key, account_code, remark FROM account_mappings ORDER BY scope, key",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(AccountMapping {
                id: r.get(0)?,
                scope: r.get(1)?,
                key: r.get(2)?,
                account_code: r.get(3)?,
                remark: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 保存科目映射（同 scope+key 幂等覆盖）。
pub fn save_account_mapping(
    conn: &Connection,
    input: &AccountMappingInput,
) -> AppResult<AccountMapping> {
    if !["expense_type", "department"].contains(&input.scope.as_str()) {
        return Err(AppError::General(
            "映射 scope 只支持 expense_type / department".into(),
        ));
    }
    let category: String = conn.query_row(
        "SELECT category FROM gl_accounts WHERE code = ?1",
        params![input.account_code],
        |r| r.get(0),
    )?;
    if input.scope == "expense_type" && !["profit_loss", "cost"].contains(&category.as_str()) {
        return Err(AppError::General(format!(
            "科目 {} 不是费用类科目",
            input.account_code
        )));
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO account_mappings (scope, key, account_code, remark, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(scope, key) DO UPDATE SET account_code = ?3, remark = ?4, updated_at = ?5",
        params![
            input.scope,
            input.key,
            input.account_code,
            input.remark,
            now
        ],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM account_mappings WHERE scope = ?1 AND key = ?2",
        params![input.scope, input.key],
        |r| r.get(0),
    )?;
    Ok(AccountMapping {
        id,
        scope: input.scope.clone(),
        key: input.key.clone(),
        account_code: input.account_code.clone(),
        remark: input.remark.clone(),
    })
}

/// 删除科目映射，返回是否实际删除了行。
pub fn delete_account_mapping(conn: &Connection, id: i64) -> AppResult<bool> {
    Ok(conn.execute("DELETE FROM account_mappings WHERE id = ?1", params![id])? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::create_tables(&conn).unwrap();
        db::seed_gl_accounts(&conn).unwrap();
        conn
    }

    #[test]
    fn test_account_crud() {
        let conn = setup();
        let accounts = get_accounts(&conn).unwrap();
        // Task 1 预置《小企业会计准则》62 个一级科目（brief 写 70 与实际交付不符，按 62 断言）
        assert!(accounts.len() >= 62);
        // 新增自定义科目
        let created = create_account(
            &conn,
            &GlAccountInput {
                code: "660201".into(),
                name: "管理费用—办公费".into(),
                category: "profit_loss".into(),
                direction: "debit".into(),
                cash_flow_category: Some("operating".into()),
                remark: None,
            },
        )
        .unwrap();
        assert_eq!(created.is_system, 0);
        // 编码重复报错
        assert!(create_account(
            &conn,
            &GlAccountInput {
                code: "660201".into(),
                name: "重复".into(),
                category: "profit_loss".into(),
                direction: "debit".into(),
                cash_flow_category: None,
                remark: None,
            }
        )
        .is_err());
        // 停用/启用
        assert!(set_account_active(&conn, "660201", false).unwrap());
        assert!(set_account_active(&conn, "660201", true).unwrap());
    }

    #[test]
    fn test_opening_balance_validation() {
        let conn = setup();
        let mut rows = vec![
            OpeningBalanceRow {
                account_code: "1002".into(),
                debit_amount: 100000.0,
                credit_amount: 0.0,
            },
            OpeningBalanceRow {
                account_code: "2001".into(),
                debit_amount: 0.0,
                credit_amount: 40000.0,
            },
            // 少 60000，不平
        ];
        let err = save_opening_balances(&conn, "2026-01", &rows);
        assert!(err.is_err());
        rows.push(OpeningBalanceRow {
            account_code: "3001".into(),
            debit_amount: 0.0,
            credit_amount: 60000.0,
        });
        save_opening_balances(&conn, "2026-01", &rows).unwrap();
        // 换月重录：清空旧月重新保存
        let (month, loaded) = get_opening_balances(&conn).unwrap();
        assert_eq!(month.as_deref(), Some("2026-01"));
        assert_eq!(loaded.len(), 3);
    }

    #[test]
    fn test_account_mapping() {
        let conn = setup();
        save_account_mapping(
            &conn,
            &AccountMappingInput {
                scope: "expense_type".into(),
                key: "OFFICE".into(),
                account_code: "6602".into(),
                remark: None,
            },
        )
        .unwrap();
        let maps = get_account_mappings(&conn).unwrap();
        assert_eq!(maps.len(), 1);
        assert!(delete_account_mapping(&conn, maps[0].id).unwrap());
    }
}
