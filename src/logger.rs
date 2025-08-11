use std::{ fs::{ self, File, OpenOptions }, io::Write, path::{ Path }, sync::{ Arc, Mutex } };

use chrono::Local;
use directories::BaseDirs;

use crate::plugin::PLUGIN_UUID;

const MAX_LOG_ROTATIONS: usize = 3;

#[macro_export]
macro_rules! log {
    (
        $logger:expr,
        $($arg:tt)*
    ) => {
        {
        $logger.log(&format!($($arg)*));
        }
    };
}

pub trait ActionLog: Send + Sync {
    fn log(&self, message: &str);
}

pub struct FileLogger {
    file: Arc<Mutex<File>>,
}

impl FileLogger {
    pub fn init<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path_buf = path.as_ref().to_path_buf();
        Self::rotate_logs(&path_buf)?;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path_buf)
            .map_err(|e| format!("Failed to open log file: {e}"))?;

        Ok(Self {
            file: Arc::new(Mutex::new(file)),
        })
    }

    pub fn from_appdata() -> Result<Self, String> {
        let base = BaseDirs::new().ok_or("Could not find user data directory")?;
        let log_dir = base.data_dir().join(PLUGIN_UUID);

        fs::create_dir_all(&log_dir).map_err(|e| format!("Failed to create log directory: {e}"))?;

        let log_file = log_dir.join("plugin.log");

        Self::init(log_file)
    }

    fn rotate_logs(base_path: &Path) -> Result<(), String> {
        for i in (1..=MAX_LOG_ROTATIONS).rev() {
            let src = base_path.with_extension(format!("{i}.log"));
            let dst = base_path.with_extension(format!("{}.log", i + 1));
            if src.exists() {
                if i == MAX_LOG_ROTATIONS {
                    fs::remove_file(&src).map_err(|e| format!("Failed to remove old log: {e}"))?;
                } else {
                    fs::rename(&src, &dst).map_err(|e| format!("Failed to rotate log: {e}"))?;
                }
            }
        }

        if base_path.exists() {
            let rotated = base_path.with_extension("1.log");
            fs::rename(base_path, rotated).map_err(|e| format!("Failed to archive log: {e}"))?;
        }

        Ok(())
    }
}

impl ActionLog for FileLogger {
    fn log(&self, message: &str) {
        let timestamp = Local::now().format("[%Y-%m-%d %H:%M:%S]").to_string();
        let formatted = format!("{timestamp} {message}\n");

        let mut file = self.file.lock().unwrap();
        if let Err(e) = file.write_all(formatted.as_bytes()) {
            eprintln!("Failed to write to log file: {e}");
        }
    }
}
