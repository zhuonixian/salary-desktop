//! 第十一阶段：提醒触达（spec 2026-10-12-stage11）
//!
//! 职责：把散落在四处的提醒（办税征期 / 账期借款票据滞留应付 / 月结 / 备份）
//! 聚合为统一结构（稳定 key + 不含金额的 title/body），供前端 OS 系统通知检查器
//! 消费；本地通知设置与"每条每天最多弹一次"的去重记忆。
//!
//! 隐私口径：通知文本对齐 stage9 OperationLogs detail 口径——不含金额等敏感数字。

use chrono::Datelike;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

// ==================== 聚合提醒 ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DueNotification {
    /// 稳定去重键："{kind}:{唯一标识}:{关键日期}"
    pub key: String,
    /// 类别：tax_filing / advance_due / instrument_due / payable_stuck / month_close / backup
    pub kind: String,
    /// 展示标题（不含金额）
    pub title: String,
    /// 通知正文（不含金额等敏感数字）
    pub body: String,
    pub due_date: Option<String>,
    /// overdue（已逾期）| soon（临近，≤3 天）| info
    pub level: String,
}

fn level_for_days_left(days_left: i64) -> &'static str {
    if days_left < 0 {
        "overdue"
    } else if days_left <= 3 {
        "soon"
    } else {
        "info"
    }
}

/// 征期源：窗口为「上月起 2 个月」——次月 15 日申报期次的到期倒计时只有从
/// 上月起算才会落入 15 天提醒窗（从当月起算会整月漏报）；pending 且
/// （已逾期或 15 日内到期）。空账套跳过——全新安装尚无申报义务，零打扰。
fn collect_tax_filing(
    conn: &Connection,
    today: chrono::NaiveDate,
) -> AppResult<Vec<DueNotification>> {
    if crate::onboarding::is_empty_ledger(conn)? {
        return Ok(Vec::new());
    }
    let first = today
        .with_day(1)
        .ok_or_else(|| AppError::General("日期越界".into()))?;
    let prev_first = first
        .checked_sub_months(chrono::Months::new(1))
        .ok_or_else(|| AppError::General("月份越界".into()))?;
    let from_month = prev_first.format("%Y-%m").to_string();
    let items = crate::onboarding::get_tax_calendar(conn, &from_month, 2)?;
    let mut out = Vec::new();
    for item in items {
        if item.status != "pending" || item.days_left > 15 {
            continue;
        }
        let when = if item.days_left < 0 {
            format!("已逾期 {} 天", -item.days_left)
        } else {
            format!("剩 {} 天", item.days_left)
        };
        let mut body = format!("{}：截止 {}（{when}）", item.label, item.due_date);
        if let Some(hint) = &item.hint {
            body.push_str(&format!("；{hint}"));
        }
        out.push(DueNotification {
            key: format!("tax_filing:{}:{}", item.kind, item.period),
            kind: "tax_filing".into(),
            title: format!("征期提醒：{}", item.label),
            body,
            due_date: Some(item.due_date),
            level: if item.overdue {
                "overdue".into()
            } else if item.days_left <= 3 {
                "soon".into()
            } else {
                "info".into()
            },
        });
    }
    Ok(out)
}

/// 账期源：借款到期 / 票据到期 / 滞留应付（复用仪表盘提醒卡查询，内部已按提前天数过滤）。
fn collect_due_items(
    conn: &Connection,
    today: chrono::NaiveDate,
) -> AppResult<Vec<DueNotification>> {
    let today_str = today.format("%Y-%m-%d").to_string();
    let rows = crate::db::get_dashboard_reminders(conn, &today_str)?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let (kind, body) = match r.category.as_str() {
                "advance_due" => (
                    "advance_due",
                    format!("员工借款 {} 将于 {} 到期", r.title, r.due_date),
                ),
                "instrument_due" => (
                    "instrument_due",
                    format!("票据 {} 将于 {} 到期", r.title, r.due_date),
                ),
                _ => (
                    "payable_stuck",
                    format!("滞留应付 {} 已滞留 {} 天", r.title, r.days_left),
                ),
            };
            let level = if r.category == "payable_stuck" {
                "info"
            } else {
                level_for_days_left(r.days_left)
            };
            DueNotification {
                key: format!("{}:{}:{}", kind, r.ref_id, r.due_date),
                kind: kind.into(),
                title: r.title,
                body,
                due_date: Some(r.due_date),
                level: level.into(),
            }
        })
        .collect())
}

/// 月结源：上月有活动（锁定工资或凭证）但未正式月结。
fn collect_month_close(
    conn: &Connection,
    today: chrono::NaiveDate,
) -> AppResult<Vec<DueNotification>> {
    let first = today
        .with_day(1)
        .ok_or_else(|| AppError::General("日期越界".into()))?;
    let prev_first = first
        .checked_sub_months(chrono::Months::new(1))
        .ok_or_else(|| AppError::General("月份越界".into()))?;
    let month = prev_first.format("%Y-%m").to_string();

    if crate::db::is_month_closed(conn, &month)? {
        return Ok(Vec::new());
    }
    let locked: i64 = conn.query_row(
        "SELECT COUNT(*) FROM salary_monthly_results WHERE salary_month = ?1 AND locked = 1",
        params![month],
        |r| r.get(0),
    )?;
    let vouchers: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vouchers WHERE belong_month = ?1",
        params![month],
        |r| r.get(0),
    )?;
    if locked == 0 && vouchers == 0 {
        return Ok(Vec::new());
    }
    let level = if today.day() > 15 { "overdue" } else { "soon" };
    Ok(vec![DueNotification {
        key: format!("month_close:{month}"),
        kind: "month_close".into(),
        title: format!("{month} 月结提醒"),
        body: format!("{month} 存在已锁定工资或记账凭证，尚未正式月结，请完成月结体检并关账"),
        due_date: None,
        level: level.into(),
    }])
}

/// 备份源：非空账套且（从未备份或距上次备份超 30 天）。
fn collect_backup(conn: &Connection) -> AppResult<Vec<DueNotification>> {
    if crate::onboarding::is_empty_ledger(conn)? {
        return Ok(Vec::new());
    }
    let last = crate::db::get_setting(conn, "last_data_backup_at")?;
    let days = match last {
        Some(ts) => match chrono::DateTime::parse_from_rfc3339(&ts) {
            Ok(t) => {
                let days = (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_days();
                if days <= 30 {
                    return Ok(Vec::new());
                }
                days
            }
            Err(_) => {
                // 时间戳损坏视为从未备份
                i64::MAX
            }
        },
        None => i64::MAX,
    };
    let body = if days == i64::MAX {
        "尚未创建过数据备份，建议在数据安全中心创建加密备份包并转移异机保存".into()
    } else {
        format!("距上次备份已超过 {days} 天，建议在数据安全中心创建新的备份包")
    };
    Ok(vec![DueNotification {
        key: format!(
            "backup:{}",
            chrono::Local::now().date_naive().format("%Y-%m-%d")
        ),
        kind: "backup".into(),
        title: "数据备份提醒".into(),
        body,
        due_date: None,
        level: "info".into(),
    }])
}

/// 聚合四源提醒：征期 → 账期 → 月结 → 备份；按 level（overdue 优先）与截止日排序。
pub fn get_due_notifications(conn: &Connection) -> AppResult<Vec<DueNotification>> {
    let today = chrono::Local::now().date_naive();
    let mut items = Vec::new();
    items.extend(collect_tax_filing(conn, today)?);
    items.extend(collect_due_items(conn, today)?);
    items.extend(collect_month_close(conn, today)?);
    items.extend(collect_backup(conn)?);
    items.sort_by(|a, b| {
        let la = level_rank(&a.level);
        let lb = level_rank(&b.level);
        la.cmp(&lb).then_with(|| a.due_date.cmp(&b.due_date))
    });
    Ok(items)
}

fn level_rank(level: &str) -> u8 {
    match level {
        "overdue" => 0,
        "soon" => 1,
        _ => 2,
    }
}

// ==================== 本地通知设置 ====================

pub const LOCAL_NOTIFY_ENABLED_KEY: &str = "local_notify_enabled";
pub const CLOSE_TO_TRAY_KEY: &str = "close_to_tray_enabled";

fn read_bool_setting(conn: &Connection, key: &str, default: bool) -> AppResult<bool> {
    Ok(match crate::db::get_setting(conn, key)? {
        Some(v) => v != "0",
        None => default,
    })
}

#[derive(Debug, Serialize)]
pub struct LocalNotifySettings {
    pub enabled: bool,
    pub close_to_tray: bool,
}

/// 保存本地通知设置的前端入参（部分保存：None 不修改）
#[derive(Debug, Deserialize)]
pub struct LocalNotifySettingsInput {
    pub enabled: Option<bool>,
    pub close_to_tray: Option<bool>,
}

pub fn get_local_notify_settings(conn: &Connection) -> AppResult<LocalNotifySettings> {
    Ok(LocalNotifySettings {
        enabled: read_bool_setting(conn, LOCAL_NOTIFY_ENABLED_KEY, true)?,
        close_to_tray: read_bool_setting(conn, CLOSE_TO_TRAY_KEY, true)?,
    })
}

pub fn save_local_notify_settings(
    conn: &Connection,
    enabled: Option<bool>,
    close_to_tray: Option<bool>,
) -> AppResult<()> {
    if let Some(v) = enabled {
        crate::db::set_setting(conn, LOCAL_NOTIFY_ENABLED_KEY, if v { "1" } else { "0" })?;
    }
    if let Some(v) = close_to_tray {
        crate::db::set_setting(conn, CLOSE_TO_TRAY_KEY, if v { "1" } else { "0" })?;
    }
    Ok(())
}

/// 关窗行为判定（CloseRequested 时由 lib.rs 调用）：默认关窗到托盘，读设置失败保守驻留。
pub fn should_close_to_tray(conn: &Connection) -> bool {
    read_bool_setting(conn, CLOSE_TO_TRAY_KEY, true).unwrap_or(true)
}

// ==================== 托盘 badge ====================

/// 刷新托盘 tooltip 待办数（MAIN_TRAY_ID）；无托盘环境（无头 Linux 等）静默忽略。
pub const MAIN_TRAY_ID: &str = "main-tray";
pub const APP_NAME: &str = "工资核算助手";

pub fn update_tray_badge(app: &tauri::AppHandle, count: i64) {
    if let Some(tray) = app.tray_by_id(MAIN_TRAY_ID) {
        let tip = if count > 0 {
            format!("{APP_NAME}（{count} 项提醒待处理）")
        } else {
            APP_NAME.to_string()
        };
        let _ = tray.set_tooltip(Some(&tip));
    }
}

// ==================== 提醒记忆（每条每天至多提醒一次） ====================

pub const NOTIFY_MEMORY_KEY: &str = "local_notify_memory";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyMemory {
    pub date: String,
    pub keys: Vec<String>,
}

fn today_str() -> String {
    chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

/// 读取今日提醒记忆；跨日自动重置为空（新的一天重新提醒）；损坏视为空集合重建。
pub fn get_notify_memory(conn: &Connection) -> AppResult<NotifyMemory> {
    let today = today_str();
    let memory: Option<NotifyMemory> = crate::db::get_setting(conn, NOTIFY_MEMORY_KEY)?
        .and_then(|json| serde_json::from_str(&json).ok());
    Ok(match memory {
        Some(m) if m.date == today => m,
        _ => NotifyMemory {
            date: today,
            keys: Vec::new(),
        },
    })
}

/// 合并写入今日已提醒 keys（幂等，超日重置）。
pub fn mark_notified(conn: &Connection, keys: &[String]) -> AppResult<()> {
    if keys.is_empty() {
        return Ok(());
    }
    let mut memory = get_notify_memory(conn)?;
    for key in keys {
        if !memory.keys.contains(key) {
            memory.keys.push(key.clone());
        }
    }
    crate::db::set_setting(conn, NOTIFY_MEMORY_KEY, &serde_json::to_string(&memory)?)?;
    Ok(())
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::seed_gl_accounts(&conn).unwrap();
        crate::db::insert_default_data(&conn).unwrap();
        conn
    }

    /// 非空账套构造：1 名员工（有锁定上月工资 + 当月凭证由业务生成太重，直接插行）
    fn seed_one_employee(conn: &Connection, locked_month: &str) -> i64 {
        conn.execute(
            "INSERT INTO employees (employee_no, name, status, base_salary, created_at, updated_at)
             VALUES ('T001', '测试员工', 'active', 5000, '2026-01-01', '2026-01-01')",
            [],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO salary_monthly_results
             (salary_month, employee_no, name, gross_salary, net_salary, status, locked, created_at, updated_at)
             VALUES (?1, 'T001', '测试员工', 5000, 4500, '已锁定', 1, '2026-01-01', '2026-01-01')",
            params![locked_month],
        )
        .unwrap();
        id
    }

    #[test]
    fn empty_ledger_produces_no_notifications() {
        let conn = setup();
        // 空账套：征期被跳过、账期/月结/备份无数据 —— 零打扰
        let items = get_due_notifications(&conn).unwrap();
        assert!(items.is_empty(), "空账套应零提醒，实际: {items:?}");
    }

    #[test]
    fn tax_filing_source_with_activity() {
        let conn = setup();
        let prev_month = chrono::Local::now()
            .date_naive()
            .with_day(1)
            .unwrap()
            .checked_sub_months(chrono::Months::new(1))
            .unwrap()
            .format("%Y-%m")
            .to_string();
        seed_one_employee(&conn, &prev_month);

        let items = get_due_notifications(&conn).unwrap();
        // 个税按月征期默认启用：本月期次（上月工资）days_left = 到次月 15 日，必 >0；
        // 视窗 2 个月，第二条期次截止更远可能 >15 天被过滤
        let tax: Vec<&DueNotification> = items.iter().filter(|i| i.kind == "tax_filing").collect();
        assert!(!tax.is_empty(), "有活动账套应产生征期提醒");
        assert!(tax[0].key.starts_with("tax_filing:eit_withholding:"));
        assert!(tax[0].title.contains("个人所得税扣缴申报"));
        assert!(tax[0].body.contains("截止"));
        // 上月有锁定工资 → hint 提示已锁定
        assert!(tax[0].body.contains("已锁定"));
    }

    #[test]
    fn month_close_source_hit_and_miss() {
        let conn = setup();
        let prev_month = chrono::Local::now()
            .date_naive()
            .with_day(1)
            .unwrap()
            .checked_sub_months(chrono::Months::new(1))
            .unwrap()
            .format("%Y-%m")
            .to_string();
        seed_one_employee(&conn, &prev_month);

        let items = get_due_notifications(&conn).unwrap();
        let mc: Vec<&DueNotification> = items.iter().filter(|i| i.kind == "month_close").collect();
        assert_eq!(mc.len(), 1, "上月有锁定工资未月结 → 提醒");
        assert_eq!(mc[0].key, format!("month_close:{prev_month}"));
        assert!(mc[0].body.contains("尚未正式月结"));

        // 正式月结后不再提醒
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO month_closes (month, status, closed_at, closed_by, created_at, updated_at)
             VALUES (?1, 'closed', ?2, '测试', ?2, ?2)",
            params![prev_month, now],
        )
        .unwrap();
        let items = get_due_notifications(&conn).unwrap();
        assert!(!items.iter().any(|i| i.kind == "month_close"));

        // 无活动月份（未结也不提醒）
        conn.execute("DELETE FROM month_closes", []).unwrap();
        conn.execute("DELETE FROM salary_monthly_results", [])
            .unwrap();
        let items = get_due_notifications(&conn).unwrap();
        assert!(!items.iter().any(|i| i.kind == "month_close"));
    }

    #[test]
    fn backup_source_threshold() {
        let conn = setup();
        seed_one_employee(&conn, "2026-08");

        // 从未备份 → 提醒
        let items = get_due_notifications(&conn).unwrap();
        let bk: Vec<&DueNotification> = items.iter().filter(|i| i.kind == "backup").collect();
        assert_eq!(bk.len(), 1);
        assert!(bk[0].body.contains("尚未创建过数据备份"));

        // 31 天前备份 → 提醒（正文含实际天数）
        let old = (chrono::Utc::now() - chrono::Duration::days(31)).to_rfc3339();
        crate::db::set_setting(&conn, "last_data_backup_at", &old).unwrap();
        let items = get_due_notifications(&conn).unwrap();
        let bk: Vec<&DueNotification> = items.iter().filter(|i| i.kind == "backup").collect();
        assert_eq!(bk.len(), 1);
        assert!(bk[0].body.contains("超过 31 天"), "实际: {}", bk[0].body);

        // 10 天前备份 → 不提醒
        let recent = (chrono::Utc::now() - chrono::Duration::days(10)).to_rfc3339();
        crate::db::set_setting(&conn, "last_data_backup_at", &recent).unwrap();
        let items = get_due_notifications(&conn).unwrap();
        assert!(!items.iter().any(|i| i.kind == "backup"));

        // 时间戳损坏 → 视为从未备份
        crate::db::set_setting(&conn, "last_data_backup_at", "not-a-date").unwrap();
        let items = get_due_notifications(&conn).unwrap();
        assert!(items.iter().any(|i| i.kind == "backup"));
    }

    #[test]
    fn due_items_source_keys_and_level() {
        let conn = setup();
        seed_one_employee(&conn, "2026-08");
        // 借款：已结算（放款后到期待收回才纳入提醒）、到期日 3 天后（soon）
        let due = (chrono::Local::now().date_naive() + chrono::Duration::days(3))
            .format("%Y-%m-%d")
            .to_string();
        conn.execute(
            "INSERT INTO fund_documents
             (document_no, document_type, belong_month, document_date, amount, summary, status,
              employee_id, due_date, created_at, updated_at)
             VALUES ('JK20260801', 'advance', '2026-08', '2026-08-01', 600, '备用金',
                     'settled', 1, ?1, '2026-08-01', '2026-08-01')",
            params![due],
        )
        .unwrap();

        let items = get_due_notifications(&conn).unwrap();
        let adv: Vec<&DueNotification> = items.iter().filter(|i| i.kind == "advance_due").collect();
        assert_eq!(adv.len(), 1);
        assert_eq!(adv[0].key, format!("advance_due:1:{due}"));
        assert_eq!(adv[0].level, "soon");
        // 通知正文不含金额数字
        assert!(
            !adv[0].body.contains("600"),
            "通知正文不得含金额: {}",
            adv[0].body
        );
        assert!(adv[0].body.contains("将于") && adv[0].body.contains("到期"));
    }

    #[test]
    fn settings_roundtrip_and_close_to_tray_default() {
        let conn = setup();
        // 默认双开
        let s = get_local_notify_settings(&conn).unwrap();
        assert!(s.enabled && s.close_to_tray);
        // 关闭后读回
        save_local_notify_settings(&conn, Some(false), Some(false)).unwrap();
        let s = get_local_notify_settings(&conn).unwrap();
        assert!(!s.enabled && !s.close_to_tray);
        // 关窗判定读设置
        assert!(!should_close_to_tray(&conn));
        // 部分保存：只改 enabled
        save_local_notify_settings(&conn, Some(true), None).unwrap();
        let s = get_local_notify_settings(&conn).unwrap();
        assert!(s.enabled && !s.close_to_tray);
    }

    #[test]
    fn notify_memory_cross_day_reset_and_merge() {
        let conn = setup();
        // 初始为空
        let m = get_notify_memory(&conn).unwrap();
        assert!(m.keys.is_empty());
        assert_eq!(
            m.date,
            chrono::Local::now()
                .date_naive()
                .format("%Y-%m-%d")
                .to_string()
        );

        // 合并写入 + 幂等
        mark_notified(&conn, &["k1".into(), "k2".into()]).unwrap();
        mark_notified(&conn, &["k2".into(), "k3".into()]).unwrap();
        let m = get_notify_memory(&conn).unwrap();
        assert_eq!(m.keys, vec!["k1", "k2", "k3"]);

        // 跨日重置：把日期改写为昨天
        let yesterday = (chrono::Local::now().date_naive() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let stale = NotifyMemory {
            date: yesterday,
            keys: vec!["old".into()],
        };
        crate::db::set_setting(
            &conn,
            NOTIFY_MEMORY_KEY,
            &serde_json::to_string(&stale).unwrap(),
        )
        .unwrap();
        let m = get_notify_memory(&conn).unwrap();
        assert!(m.keys.is_empty(), "跨日应自动重置");
        // 空合并不写入
        mark_notified(&conn, &[]).unwrap();

        // 损坏 JSON 视为空
        crate::db::set_setting(&conn, NOTIFY_MEMORY_KEY, "broken{").unwrap();
        let m = get_notify_memory(&conn).unwrap();
        assert!(m.keys.is_empty());
    }

    #[test]
    fn sort_overdue_first() {
        let conn = setup();
        let prev_month = chrono::Local::now()
            .date_naive()
            .with_day(1)
            .unwrap()
            .checked_sub_months(chrono::Months::new(1))
            .unwrap()
            .format("%Y-%m")
            .to_string();
        seed_one_employee(&conn, &prev_month);
        // 已逾期借款（due 5 天前，已结算）
        let past = (chrono::Local::now().date_naive() - chrono::Duration::days(5))
            .format("%Y-%m-%d")
            .to_string();
        conn.execute(
            "INSERT INTO fund_documents
             (document_no, document_type, belong_month, document_date, amount, summary, status,
              employee_id, due_date, created_at, updated_at)
             VALUES ('JK20260701', 'advance', '2026-07', '2026-07-01', 800, '备用金',
                     'settled', 1, ?1, '2026-07-01', '2026-07-01')",
            params![past],
        )
        .unwrap();

        let items = get_due_notifications(&conn).unwrap();
        let ranks: Vec<u8> = items.iter().map(|i| level_rank(&i.level)).collect();
        let mut sorted = ranks.clone();
        sorted.sort_unstable();
        assert_eq!(ranks, sorted, "overdue 应排在最前：{items:?}");
    }
}
