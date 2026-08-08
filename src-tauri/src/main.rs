// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;

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

fn main() {
    // Set panic hook to log panics
    std::panic::set_hook(Box::new(|info| {
        diag(&format!("PANIC: {info}"));
    }));

    diag("main() started");
    diag(&format!("exe: {:?}", std::env::current_exe()));
    diag(&format!("cwd: {:?}", std::env::current_dir()));

    diag("calling app_lib::run()");
    app_lib::run();
    diag("app_lib::run() returned (unexpected)");
}
