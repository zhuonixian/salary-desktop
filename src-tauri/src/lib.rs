mod accounting;
mod commands;
mod data_safety;
mod db;
mod errors;
mod excel;
mod invoice;
mod legacy_migration;
mod models;
mod ocr;
mod salary;
pub mod security;
mod security_commands;

use std::io::Write;
use std::sync::Mutex;
use tauri::Manager;

fn diag(msg: &str) {
    let tmp_log = std::env::temp_dir().join("salary-desktop-startup.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&tmp_log)
    {
        let _ = writeln!(f, "[{}] {}", chrono::Utc::now().format("%H:%M:%S"), msg);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    diag("lib::run() entered");

    diag("creating Tauri Builder...");
    let mut builder = tauri::Builder::default();

    diag("adding dialog plugin...");
    builder = builder.plugin(tauri_plugin_dialog::init());

    diag("adding fs plugin...");
    builder = builder.plugin(tauri_plugin_fs::init());

    diag("adding log plugin...");
    builder = builder.plugin(
        tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .targets([
                tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                    file_name: None,
                }),
                tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
            ])
            .build(),
    );

    diag("setting up setup callback...");
    builder = builder.setup(|app| {
        diag("setup() callback entered");

        // 启动时清理上次崩溃可能残留的发票预览目录（仅清理我们自己的子目录，
        // 不触碰 temp_dir 顶层其它内容）。
        let preview_dir = std::env::temp_dir().join("salary-desktop-preview");
        if preview_dir.exists() {
            diag(&format!(
                "cleaning stale preview dir: {}",
                preview_dir.display()
            ));
            if let Err(e) = std::fs::remove_dir_all(&preview_dir) {
                diag(&format!("WARNING: remove_dir_all(preview_dir) failed: {e}"));
            }
        }

        // Initialize database
        let app_data_dir = match app.path().app_data_dir() {
            Ok(dir) => {
                diag(&format!("app_data_dir: {}", dir.display()));
                dir
            }
            Err(e) => {
                diag(&format!("ERROR: app_data_dir failed: {e}"));
                return Err(Box::new(e) as Box<dyn std::error::Error>);
            }
        };

        if let Err(e) = std::fs::create_dir_all(&app_data_dir) {
            diag(&format!("WARNING: create_dir_all failed: {e}"));
        }

        let db_path = app_data_dir.to_str().unwrap_or("salary.db");
        diag(&format!("initializing database at: {db_path}"));

        let conn = match db::init_db(db_path) {
            Ok(c) => {
                diag("database initialized OK");
                c
            }
            Err(e) => {
                diag(&format!("ERROR: database init failed: {e}"));
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Database init failed: {e}"),
                )) as Box<dyn std::error::Error>);
            }
        };

        app.manage(Mutex::new(conn));
        app.manage(security::SecurityState::new());
        diag("setup() complete, all OK");
        Ok(())
    });

    diag("registering invoke handler...");
    builder = builder.invoke_handler(tauri::generate_handler![
        commands::get_employees,
        commands::get_employee,
        commands::create_employee,
        commands::update_employee,
        commands::delete_employee,
        commands::search_employees,
        commands::import_employees_excel,
        commands::export_employee_import_template,
        commands::get_attendance_records,
        commands::import_attendance_excel,
        commands::export_attendance_import_template,
        commands::save_attendance_records,
        commands::create_attendance_record,
        commands::update_attendance_record,
        commands::delete_attendance_record,
        commands::get_salary_rules,
        commands::update_salary_rule,
        commands::get_tax_rules,
        commands::update_tax_rule,
        commands::calculate_salary,
        commands::get_salary_results,
        commands::update_salary_result,
        commands::lock_salary_results,
        commands::review_salary_results,
        commands::recalculate_employee,
        commands::ocr_recognize,
        commands::get_ocr_batches,
        commands::confirm_ocr_results,
        commands::export_salary_detail,
        commands::export_bank_payment_file,
        commands::export_salary_slips,
        commands::export_attendance_summary_file,
        commands::get_dashboard_summary,
        commands::get_month_close_workbench,
        commands::get_month_close_status,
        commands::close_month,
        commands::reopen_month,
        commands::get_financial_analysis,
        commands::export_department_cost_report,
        commands::export_expense_analysis_report,
        commands::export_month_close_report,
        commands::export_month_close_package,
        commands::query_payment_batches,
        commands::get_payment_batch_detail,
        commands::create_payment_batch,
        commands::export_payment_batch_file,
        commands::mark_payment_batch_paid,
        commands::void_payment_batch,
        commands::update_payment_batch_remark,
        commands::import_bank_transactions_file,
        commands::query_bank_transactions,
        commands::auto_match_bank_transactions,
        commands::confirm_bank_transaction_match,
        commands::cancel_bank_transaction_match,
        commands::ignore_bank_transaction,
        commands::query_budgets,
        commands::save_budget,
        commands::delete_budget,
        commands::query_operation_logs,
        commands::get_data_safety_status,
        commands::backup_database,
        commands::restore_database,
        commands::verify_database,
        commands::compact_database,
        commands::open_app_data_dir,
        commands::get_ocr_settings,
        commands::save_ocr_settings,
        commands::generate_punch_card_template,
        commands::ocr_recognize_punch_card,
        commands::get_invoice_expense_types,
        commands::save_invoice_expense_type,
        commands::delete_invoice_expense_type,
        commands::ocr_invoice,
        commands::save_invoice,
        commands::update_invoice,
        commands::delete_invoice,
        commands::query_invoices,
        commands::export_invoice_list,
        commands::query_reimbursement_claims,
        commands::save_reimbursement_claim,
        commands::get_reimbursement_invoices,
        commands::update_reimbursement_claim_status,
        commands::delete_reimbursement_claim,
        // ===== Accounting（第五阶段 科目/期初/映射） =====
        commands::get_gl_accounts,
        commands::create_gl_account,
        commands::set_gl_account_active,
        commands::get_opening_balances,
        commands::save_opening_balances,
        commands::get_account_mappings,
        commands::save_account_mapping,
        commands::delete_account_mapping,
        // ===== Accounting（第五阶段 凭证查询/银行流水凭证） =====
        commands::get_vouchers,
        commands::create_bank_manual_voucher,
        // ===== Accounting（第五阶段 报表命令/导出） =====
        commands::get_balance_sheet,
        commands::get_income_statement,
        commands::get_cash_flow_statement,
        commands::export_financial_report,
        commands::get_trial_balance,
        commands::export_trial_balance,
        // ===== 个税年度汇总（第六阶段 Task 10） =====
        commands::get_annual_tax_summary,
        commands::export_annual_tax_summary,
        // ===== 社保公积金台账（第六阶段 Task 6） =====
        commands::get_social_profiles,
        commands::save_social_profile,
        commands::delete_social_profile,
        commands::copy_social_profiles,
        commands::get_social_base_limits,
        commands::set_social_base_limits,
        // ===== Security（Task 6） =====
        security_commands::is_security_initialized,
        security_commands::setup_security,
        security_commands::unlock,
        security_commands::lock,
        security_commands::get_security_status,
        security_commands::change_password,
        security_commands::reset_password_by_recovery,
        security_commands::reset_password_by_question,
        security_commands::update_idle_settings,
        security_commands::update_sensitive_reveal_settings,
        security_commands::reveal_sensitive_data,
        security_commands::unlock_salary_results,
        security_commands::get_legacy_migration_status,
        security_commands::migrate_legacy_resources,
        security_commands::get_decrypted_invoice_url,
    ]);

    diag("calling builder.run()...");
    match builder.run(tauri::generate_context!()) {
        Ok(_) => diag("builder.run() returned normally"),
        Err(e) => diag(&format!("builder.run() ERROR: {e}")),
    }

    // 应用退出后清理发票预览目录，避免临时解密文件长期堆积。
    let preview_dir = std::env::temp_dir().join("salary-desktop-preview");
    if preview_dir.exists() {
        diag("cleaning preview dir on exit");
        let _ = std::fs::remove_dir_all(&preview_dir);
    }
}
