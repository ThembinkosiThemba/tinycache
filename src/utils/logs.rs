use crate::db::db::TinyCache;
use chrono::{DateTime, Local};
use colored::*;
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncBufReadExt, AsyncWriteExt, BufWriter},
    sync::RwLock,
};

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    System,
    Application,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Local>,
    pub level: LogLevel,
    pub log_type: LogType,
    pub database: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogType {
    Info,
    Error,
    Warn,
}

impl LogEntry {
    pub fn new(level: LogLevel, log_type: LogType, database: String, message: String) -> Self {
        Self {
            timestamp: Local::now(),
            level,
            log_type,
            database,
            message,
        }
    }
}

/// A file-based logger for recording system and application events.
///
/// The `Logger` manages log entries in a dedicated directory under the `TinyCache` data directory,
/// writing to a `current.log` file and rotating it when it exceeds a size threshold. It also
/// periodically cleans up old logs based on retention period and total size limits.
#[derive(Clone)]
pub struct Logger {
    log_dir: PathBuf, // Base directory for logs (data_dir/logs)
    current_file: Arc<RwLock<BufWriter<File>>>, // Current log file writer
    max_file_size: u64, // Max size before rotation (10MB)
    retention_period: Duration, // Retention period (30 days)
}

impl Logger {
    /// Initializes a new file-based logger instance.
    ///
    /// Creates a `logs` subdirectory under the provided base path and sets up the initial
    /// `current.log` file for writing. Starts a background task for periodic log cleanup.
    ///
    /// # Arguments
    /// - `base_path`: The base directory (typically `TinyCache`'s `data_dir`) where the `logs`
    ///   subdirectory will be created.
    ///
    /// # Returns
    /// - `io::Result<Self>`: A new `Logger` instance on success, or an `io::Error` if directory
    ///   creation or file opening fails.
    ///
    /// # Behavior
    /// - Creates `base_path/logs/` if it doesn’t exist.
    /// - Opens `current.log` in append mode.
    /// - Sets default max file size to 10MB and retention period to 30 days.
    /// - Spawns a daily cleanup task.
    pub async fn new(base_path: PathBuf) -> io::Result<Self> {
        let log_dir = base_path.join(".logs");
        fs::create_dir_all(&log_dir).await?;

        let initial_file = log_dir.join("current.log");

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&initial_file)
            .await?;

        let logger = Logger {
            log_dir,
            current_file: Arc::new(RwLock::new(BufWriter::new(file))),
            max_file_size: 10 * 1024 * 1024, // 10MB
            retention_period: Duration::from_secs(30 * 24 * 60 * 60), // 30 days
        };

        println!("{}", ">>> logger initialised".dimmed());

        logger.start_cleanup_task().await;
        Ok(logger)
    }

    // Rotates the current log file when it exceeds the size limit.
    ///
    /// Closes the current `current.log` file, renames it with a timestamp, and opens a new
    /// `current.log` for future writes.
    ///
    /// # Returns
    /// - `io::Result<()>`: `Ok(())` on successful rotation, or an `io::Error` if flushing,
    ///   renaming, or file creation fails.
    ///
    /// # Behavior
    /// - Flushes the current writer to ensure all data is written.
    /// - Renames `current.log` to `log_YYYYMMDD_HHMMSS.log`.
    /// - Opens a new `current.log` in append mode.
    async fn rotate_log_file(&self) -> io::Result<()> {
        let mut writer = self.current_file.write().await;
        writer.flush().await?;

        let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
        let old_file = self.log_dir.join("current.log");
        let new_file = self.log_dir.join(format!("log_{}.log", timestamp));
        fs::rename(&old_file, &new_file).await?;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&old_file)
            .await?;
        *writer = BufWriter::new(file);

        Ok(())
    }

    /// Removes old log files based on retention period and total size constraints.
    ///
    /// Cleans up the `logs` directory by deleting files older than the retention period or
    /// if the total size exceeds a threshold, keeping disk usage manageable.
    ///
    /// # Returns
    /// - `io::Result<()>`: `Ok(())` on successful cleanup, or an `io::Error` if directory
    ///   reading or file deletion fails.
    ///
    /// # Behavior
    /// - Scans the `logs` directory for `.log` files (excluding `current.log`).
    /// - Calculates total size and sorts files by modification time.
    /// - Deletes files older than 30 days or if total size exceeds 100MB.
    async fn cleanup_old_logs(&self) -> io::Result<()> {
        let mut entries = fs::read_dir(&self.log_dir).await?;
        let threshold = Local::now() - chrono::Duration::from_std(self.retention_period).unwrap();

        let max_total_size = 100 * 1024 * 1024; // 100MB total log size limit
        let mut total_size = 0;
        let mut log_files = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "log")
                && path != self.log_dir.join("current.log")
            {
                let metadata = entry.metadata().await?;
                total_size += metadata.len();
                let modified = metadata.modified()?;
                log_files.push((path, modified));
            }
        }

        log_files.sort_by(|a, b| a.1.cmp(&b.1)); // Oldest first

        for (path, modified) in &log_files {
            let modified_datetime = chrono::DateTime::<chrono::Local>::from((*modified).clone());

            if modified_datetime < threshold || total_size > max_total_size {
                fs::remove_file(path).await?;
                total_size = total_size.saturating_sub(max_total_size / log_files.len() as u64);
                // Approximate reduction
            }
        }

        Ok(())
    }

    async fn start_cleanup_task(&self) {
        let logger = self.clone();
        tokio::spawn(async move {
            let interval = Duration::from_secs(24 * 60 * 60); // Daily cleanup
            loop {
                tokio::time::sleep(interval).await;
                if let Err(e) = logger.cleanup_old_logs().await {
                    eprintln!("Log cleanup error: {}", e);
                }
            }
        });
    }

    /// Logs a message to the current log file with contextual information.
    ///
    /// Writes a serialized `LogEntry` to the log file and prints it to the console with colored
    /// output. Checks file size before writing and rotates if necessary.
    ///
    /// # Arguments
    /// - `message`: The log message to record.
    /// - `level`: The log level (`System` or `Application`).
    /// - `log_type`: The type of log (`Info`, `Error`, or `Warn`).
    /// - `tiny_cache`: Reference to `TinyCache` for accessing the current database context.
    ///
    /// # Returns
    /// - `io::Result<()>`: `Ok(())` on successful logging, or an `io::Error` if serialization,
    ///   file writing, or rotation fails.
    ///
    /// # Behavior
    /// - Retrieves the current database from `tiny_cache`.
    /// - Creates a `LogEntry` with timestamp and context.
    /// - Rotates the log file if it exceeds `max_file_size`.
    /// - Appends the entry as a JSON line to `current.log`.
    /// - Prints formatted output to stdout.
    pub async fn log_activity(
        &self,
        message: &str,
        level: LogLevel,
        log_type: LogType,
        tiny_cache: &TinyCache,
    ) -> io::Result<()> {
        let db = {
            let current = tiny_cache.current_database.read().await;
            current.clone().unwrap_or_else(|| "system".to_string())
        };

        let log_entry = LogEntry::new(level, log_type.clone(), db, message.to_string());
        let serialized = serde_json::to_string(&log_entry)? + "\n";

        {
            let mut writer = self.current_file.write().await;
            if writer.get_ref().metadata().await?.len() >= self.max_file_size {
                drop(writer); // Release lock before rotation
                self.rotate_log_file().await?;
                writer = self.current_file.write().await; // Re-acquire lock
            }
            writer.write_all(serialized.as_bytes()).await?;
            writer.flush().await?;
        }

        let message_color = match log_type {
            LogType::Info => message.green().dimmed(),
            LogType::Error => message.red().dimmed(),
            LogType::Warn => message.yellow().dimmed(),
        };

        println!(
            "[{}] {} {} {})",
            log_entry
                .timestamp
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
                .cyan()
                .dimmed(),
            message_color,
            match level {
                LogLevel::System => "SYSTEM".red().dimmed(),
                LogLevel::Application => "APP".blue().dimmed(),
            },
            match log_type {
                LogType::Info => "INFO",
                LogType::Error => "ERROR",
                LogType::Warn => "WARN",
            }
        );

        Ok(())
    }

    pub async fn log_info(
        &self,
        message: &str,
        level: LogLevel,
        tiny_cache: &TinyCache,
    ) -> io::Result<()> {
        self.log_activity(message, level, LogType::Info, tiny_cache)
            .await
    }

    pub async fn log_error(
        &self,
        message: &str,
        level: LogLevel,
        tiny_cache: &TinyCache,
    ) -> io::Result<()> {
        self.log_activity(message, level, LogType::Error, tiny_cache)
            .await
    }

    pub async fn log_warn(
        &self,
        message: &str,
        level: LogLevel,
        tiny_cache: &TinyCache,
    ) -> io::Result<()> {
        self.log_activity(message, level, LogType::Warn, tiny_cache)
            .await
    }

    pub async fn get_logs(
        &self,
        _tiny_cache: &TinyCache,
        database: Option<String>,
        level: Option<LogLevel>,
        start_time: Option<DateTime<Local>>,
        end_time: Option<DateTime<Local>>,
    ) -> io::Result<Vec<LogEntry>> {
        let mut logs = Vec::new();
        let mut entries = fs::read_dir(&self.log_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "log") {
                let file = File::open(&path).await?;
                let reader = tokio::io::BufReader::new(file);
                let mut lines = reader.lines();

                while let Some(line) = lines.next_line().await? {
                    if let Ok(log_entry) = serde_json::from_str::<LogEntry>(&line) {
                        if let Some(ref db) = database {
                            if log_entry.database != *db {
                                continue;
                            }
                        }
                        if let Some(ref lvl) = level {
                            match (&log_entry.level, lvl) {
                                (LogLevel::Application, LogLevel::System) => continue,
                                (LogLevel::System, LogLevel::Application) => continue,
                                _ => {}
                            }
                        }
                        if let Some(start) = start_time {
                            if log_entry.timestamp < start {
                                continue;
                            }
                        }
                        if let Some(end) = end_time {
                            if log_entry.timestamp > end {
                                continue;
                            }
                        }
                        logs.push(log_entry);
                    }
                }
            }
        }
        logs.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        Ok(logs)
    }

    pub async fn view_application_logs(
        &self,
        tiny_cache: &TinyCache,
        database: &str,
    ) -> io::Result<Vec<LogEntry>> {
        self.get_logs(
            tiny_cache,
            Some(database.to_string()),
            Some(LogLevel::Application),
            None,
            None,
        )
        .await
    }
}
