use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use chrono::Local;
use tauri::{AppHandle, Emitter};

#[derive(Clone)]
pub struct Logger {
    pub path: Option<PathBuf>,
    file: Option<Arc<Mutex<File>>>,
    app_handle: Option<AppHandle>,
}

impl Logger {
    pub fn new(app_handle: Option<AppHandle>) -> Self {
        Self {
            path: None,
            file: None,
            app_handle,
        }
    }

    pub fn start(&mut self, logs_dir: &Path, prefix: &str) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(logs_dir)?;
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        let name = format!("{}-{}.log", prefix, stamp);
        let path = logs_dir.join(name);
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&path)?;

        self.path = Some(path.clone());
        self.file = Some(Arc::new(Mutex::new(file)));

        let header = format!(
            "=== HarusoraFlashTool Log Started at {} ===\n",
            Local::now().format("%Y-%m-%d %H:%M:%S")
        );
        self.write(&header);

        Ok(path)
    }

    pub fn write(&self, text: &str) {
        let stamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        let formatted = format!("[{}] {}\n", stamp, text.trim_end());

        if let Some(ref file_arc) = self.file {
            if let Ok(mut file) = file_arc.lock() {
                let _ = file.write_all(formatted.as_bytes());
                let _ = file.flush();
            }
        }

        if let Some(ref handle) = self.app_handle {
            let _ = handle.emit("log_line", text);
        }
    }

    pub fn log_step(&self, step: &str) {
        self.write(&format!("--- {} ---", step));
    }

    pub fn fatal(&self, text: &str) {
        self.write(&format!("[FATAL] {}", text));
    }
}
