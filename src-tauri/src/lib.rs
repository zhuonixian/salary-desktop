mod commands;
mod db;
mod errors;
mod excel;
mod models;
mod ocr;
mod salary;

use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // Initialize database
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to resolve app data dir");

            // Ensure the directory exists
            std::fs::create_dir_all(&app_data_dir).ok();

            let conn =
                db::init_db(app_data_dir.to_str().unwrap()).expect("Failed to initialize database");

            app.manage(Mutex::new(conn));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Employee management
            commands::get_employees,
            commands::get_employee,
            commands::create_employee,
            commands::update_employee,
            commands::delete_employee,
            commands::search_employees,
            commands::import_employees_excel,
            // Attendance management
            commands::get_attendance_records,
            commands::import_attendance_excel,
            commands::save_attendance_records,
            commands::update_attendance_record,
            // Salary rules
            commands::get_salary_rules,
            commands::update_salary_rule,
            commands::get_tax_rules,
            commands::update_tax_rule,
            // Salary calculation
            commands::calculate_salary,
            commands::get_salary_results,
            commands::update_salary_result,
            commands::lock_salary_results,
            commands::recalculate_employee,
            // OCR
            commands::ocr_recognize,
            commands::get_ocr_batches,
            commands::confirm_ocr_results,
            // Export
            commands::export_salary_detail,
            commands::export_bank_payment_file,
            commands::export_salary_slips,
            commands::export_attendance_summary_file,
            // Dashboard
            commands::get_dashboard_summary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
