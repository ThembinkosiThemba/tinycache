/// # WAL-Only Persistence Layer for TinyCache
///
/// ## Overview
/// This persistence layer for TinyCache uses a Write-Ahead Log (WAL) approach for durability,
/// optimized for simplicity and performance without the complexity of snapshots.
/// - **WAL**: Logs every write operation (create, update, delete, etc.) to segmented files for durability.
/// - **Goals**: Ensure data consistency, minimize data loss, and enable efficient recovery through WAL replay.
///
/// ## Usage
/// 1. **Configuration**:
///    - A `PersistenceConfig` is added to define WAL settings:
///      - `persist_dir`: Stores WAL segments (e.g., "data/persist/").
///      - `wal_segment_size`: Max size of a WAL file before rotation (e.g., 16MB).
///      - `wal_sync_policy`: Sync behavior ("always", "everysec", "no").
///      - `wal_max_segments`: Maximum number of WAL segments to retain per database.
///    - In `main.rs`, initialize with `PersistenceConfig::default(&data_dir)`.
/// 2. **Integration**:
///    - `TinyCache::new_with_persistence` replaces `TinyCache::new`, setting up the `PersistenceManager`.
///    - All write operations (`create_key_value`, `update_key_value`, etc.) log to the WAL before
///      modifying the in-memory cache.
/// 3. **Operation**:
///    - Each write (e.g., `SET key value`) is logged as a `WalOperation` in a segmented WAL file
///      (e.g., "wal-default-1625091234567.log").
///    - Sync policy controls durability:
///      - "always": Syncs every write (slow, safe).
///      - "everysec": Syncs every second (balanced, lose up to 1s).
///      - "no": Relies on OS (fast, risky).
///    - WAL segments rotate when they reach the configured size limit.
///
/// ## Recovery and Loading
/// 1. **Process**:
///    - On startup, `TinyCache::new_with_persistence` calls `recover_all` to restore all databases.
///    - For each database:
///      - **Replay WAL**: Scans all "wal-<db_name>-*.log" files, sorted by timestamp, and replays
///        operations (create, update, delete, etc.) to rebuild the complete in-memory state.
///      - Skips corrupted or empty WAL lines, logging errors for transparency.
/// 2. **Data Consistency**:
///    - WAL uses a binary-compatible format (JSON with newline delimiters) to preserve `DataValue`
///      types (String, Json, etc.).
///    - WAL segments are rotated atomically to prevent corruption during writes.
///    - Old WAL segments are cleaned up based on `wal_max_segments` configuration.
/// 3. **How to Enable**:
///    - Recovery is automatic on startup via `recover_all`.
///    - To inspect: Check files in `persist_dir` (e.g., "data/persist/") for WAL logs.
///
/// ## Notes
/// - **Performance**: WAL appends are fast (no seeks); segmented approach prevents single large files.
/// - **Safety**: "everysec" sync balances speed and durability, losing at most 1s of data.
/// - **Multi-Database**: Each DB has its own WAL segments, prefixed by database name.
/// - **Simplicity**: WAL-only approach eliminates snapshot complexity while maintaining durability.
/// - **Testing**: Simulate crashes, verify recovery with `view_data` to ensure data matches.
///
use crate::{
    db::db::{DataValue, TinyCache},
    utils::utils::compute_now_timestamp,
};
use dashmap::DashMap;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{io, time::Duration};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncReadExt, AsyncWriteExt},
    sync::RwLock,
};

/// Configurations for TinyCache WAL persistence settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistenceConfig {
    /// Directory for WAL files.
    pub persist_dir: PathBuf,
    /// Maximum size of a WAL segment in bytes (e.g., 16MB).
    pub wal_segment_size: u64,
    /// Sync policy for WAL: "always" (every write), "everysec" (1s), "no" (OS handles).
    pub wal_sync_policy: String,
    /// Maximum number of WAL segments to retain per database (0 = unlimited).
    pub wal_max_segments: u32,
}

impl PersistenceConfig {
    pub fn default(data_dir: &Path) -> Self {
        info!(
            "Creating default persistence configuration for directory: {:?}",
            data_dir
        );
        PersistenceConfig {
            persist_dir: data_dir.join("data"),
            wal_segment_size: 16 * 1024 * 1024,      // 16MB
            wal_sync_policy: "everysec".to_string(), // sync every second
            wal_max_segments: 10,                    // Keep last 10 segments per DB
        }
    }
}

// --- WAL Entry and Management ---
/// Represents a single write operation for the Write-Ahead Log (WAL).
#[derive(Serialize, Deserialize, Debug)]
pub enum WalOperation {
    Create {
        key: String,
        value: DataValue,
        ttl: Option<Duration>,
    },
    Update {
        key: String,
        value: DataValue,
        ttl: Option<Duration>,
    },
    Delete {
        key: String,
    },
    Increment {
        key: String,
        amount: f64,
    },
    Decrement {
        key: String,
        amount: f64,
    },
    DropDb,
}

/// A single entry in the WAL, tied to a database and timestamped.
#[derive(Serialize, Deserialize, Debug)]
pub struct WalEntry {
    pub database: String,
    pub operation: WalOperation,
    pub timestamp: u64,
}

/// Manages the Write-Ahead Log for a single database.
pub struct WalManager {
    pub current_segment: File,
    pub segment_path: PathBuf,
    pub segment_size: u64,
    pub current_size: u64,
    pub sync_policy: String,
    pub last_sync: u64, // Last sync timestamp in seconds
    pub op_count: u64,  // Number of operations in current segment
    pub db_name: String,
}

impl WalManager {
    pub async fn new(
        persist_dir: &Path,
        db_name: &str,
        segment_size: u64,
        sync_policy: String,
    ) -> io::Result<Self> {
        info!(
            "Initializing WAL manager for database '{}' with segment size {} bytes",
            db_name, segment_size
        );

        let segment_path =
            persist_dir.join(format!("wal-{}-{}.log", db_name, compute_now_timestamp()));

        debug!("Creating persist directory: {:?}", persist_dir);
        fs::create_dir_all(persist_dir).await?;

        debug!("Opening WAL segment file: {:?}", segment_path);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&segment_path)
            .await?;

        info!(
            "WAL manager successfully initialized for database '{}' at {:?}",
            db_name, segment_path
        );

        Ok(WalManager {
            current_segment: file,
            segment_path,
            segment_size,
            current_size: 0,
            sync_policy: sync_policy.clone(),
            last_sync: compute_now_timestamp() / 1000,
            op_count: 0,
            db_name: db_name.to_string(),
        })
    }

    /// Appends an operation to the WAL and handles sync based on policy.
    pub async fn append(&mut self, entry: &WalEntry) -> io::Result<()> {
        debug!(
            "Appending WAL entry for database '{}': {:?}",
            self.db_name, entry.operation
        );

        let serialized = serde_json::to_string(&entry).map_err(|e| {
            error!(
                "Failed to serialize WAL entry for database '{}': {}",
                self.db_name, e
            );
            io::Error::new(io::ErrorKind::InvalidData, e)
        })?;

        let bytes = serialized.as_bytes();

        debug!(
            "Writing {} bytes to WAL segment for database '{}'",
            bytes.len() + 1,
            self.db_name
        );
        self.current_segment.write_all(bytes).await?;
        self.current_segment.write_all(b"\n").await?;
        self.current_size += bytes.len() as u64 + 1;
        self.op_count += 1;

        // Handle sync policy
        match self.sync_policy.as_str() {
            "always" => {
                debug!(
                    "Syncing WAL immediately (always policy) for database '{}'",
                    self.db_name
                );
                self.current_segment.sync_all().await?;
                self.last_sync = compute_now_timestamp() / 1000;
            }
            "everysec" => {
                let now_secs = compute_now_timestamp() / 1000;
                if now_secs > self.last_sync {
                    debug!(
                        "Syncing WAL (everysec policy) for database '{}'",
                        self.db_name
                    );
                    self.current_segment.sync_data().await?;
                    self.last_sync = now_secs;
                }
            }
            "no" => {
                debug!(
                    "No explicit sync (OS handles) for database '{}'",
                    self.db_name
                );
            }
            _ => {
                warn!(
                    "Unknown sync policy '{}' for database '{}', defaulting to sync",
                    self.sync_policy, self.db_name
                );
                self.current_segment.sync_data().await?;
            }
        }

        // Check if segment rotation is needed
        if self.current_size >= self.segment_size {
            info!(
                "WAL segment size limit reached ({} bytes) for database '{}', rotating segment",
                self.current_size, self.db_name
            );
            self.rotate().await?;
        }

        debug!("WAL entry successfully appended for database '{}'. Current segment size: {} bytes, operations: {}", 
               self.db_name, self.current_size, self.op_count);
        Ok(())
    }

    /// Rotates to a new WAL segment file.
    pub async fn rotate(&mut self) -> io::Result<()> {
        info!(
            "Rotating WAL segment for database '{}' (current size: {} bytes, {} operations)",
            self.db_name, self.current_size, self.op_count
        );

        let timestamp = compute_now_timestamp();
        let new_path = self
            .segment_path
            .parent()
            .unwrap()
            .join(format!("wal-{}-{}.log", self.db_name, timestamp));

        debug!(
            "Syncing current WAL segment before rotation for database '{}'",
            self.db_name
        );
        self.current_segment.sync_all().await?;

        debug!("Creating new WAL segment: {:?}", new_path);
        self.current_segment = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&new_path)
            .await?;

        self.segment_path = new_path;
        self.current_size = 0;
        self.op_count = 0;

        info!(
            "WAL segment rotation completed for database '{}'. New segment: {:?}",
            self.db_name, self.segment_path
        );
        Ok(())
    }
}

/// Manages WAL persistence for all databases in TinyCache.
pub struct PersistenceManager {
    pub config: PersistenceConfig,
    pub wal_managers: Arc<DashMap<String, RwLock<WalManager>>>,
}

impl PersistenceManager {
    /// Initializes a new persistence manager
    pub async fn new(config: PersistenceConfig) -> io::Result<Self> {
        info!("Initializing persistence manager with config: {:?}", config);

        debug!("Creating persist directory: {:?}", config.persist_dir);
        fs::create_dir_all(&config.persist_dir).await?;

        info!("Persistence manager successfully initialized");
        Ok(PersistenceManager {
            config,
            wal_managers: Arc::new(DashMap::new()),
        })
    }

    /// Ensures a WAL manager exists for a database.
    pub async fn ensure_wal(&self, db_name: &str) -> io::Result<()> {
        if !self.wal_managers.contains_key(db_name) {
            info!("Creating new WAL manager for database '{}'", db_name);

            let wal = WalManager::new(
                &self.config.persist_dir,
                db_name,
                self.config.wal_segment_size,
                self.config.wal_sync_policy.clone(),
            )
            .await?;

            self.wal_managers
                .insert(db_name.to_string(), RwLock::new(wal));
            info!(
                "WAL manager created and registered for database '{}'",
                db_name
            );
        } else {
            debug!("WAL manager already exists for database '{}'", db_name);
        }
        Ok(())
    }

    /// Logs a write operation to the WAL.
    pub async fn log_operation(&self, db_name: &str, operation: WalOperation) -> io::Result<()> {
        debug!(
            "Logging operation to WAL for database '{}': {:?}",
            db_name, operation
        );

        self.ensure_wal(db_name).await?;

        let entry = WalEntry {
            database: db_name.to_string(),
            operation,
            timestamp: compute_now_timestamp(),
        };

        let wal = self.wal_managers.get(db_name).ok_or_else(|| {
            error!("WAL manager not found for database '{}'", db_name);
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("WAL manager not found for database '{}'", db_name),
            )
        })?;

        let mut wal_lock = wal.write().await;
        wal_lock.append(&entry).await?;

        // Clean up old WAL segments if needed
        drop(wal_lock); // Release lock before cleanup
        self.cleanup_old_segments(db_name).await?;

        debug!(
            "Operation successfully logged to WAL for database '{}'",
            db_name
        );
        Ok(())
    }

    /// Recovers a database from WAL segments.
    pub async fn recover(&self, db_name: &str, tinycache: &TinyCache) -> io::Result<()> {
        info!("Starting WAL recovery for database '{}'", db_name);

        // Find all WAL files for this database
        let wal_dir = &self.config.persist_dir;
        let mut read_dir = fs::read_dir(wal_dir).await?;
        let mut wal_files = Vec::new();

        debug!("Scanning for WAL files in directory: {:?}", wal_dir);
        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            if let Some(file_name) = path.file_name() {
                if let Some(name_str) = file_name.to_str() {
                    if name_str.starts_with(&format!("wal-{}-", db_name)) {
                        debug!("Found WAL file for database '{}': {:?}", db_name, path);
                        wal_files.push(path);
                    }
                }
            }
        }

        // Sort WAL files by timestamp (embedded in filename)
        wal_files.sort();
        info!(
            "Found {} WAL segments for database '{}'",
            wal_files.len(),
            db_name
        );

        let mut total_operations = 0;
        let mut skipped_operations = 0;

        // Replay all WAL segments in order
        for (index, path) in wal_files.iter().enumerate() {
            info!(
                "Replaying WAL segment {}/{} for database '{}': {:?}",
                index + 1,
                wal_files.len(),
                db_name,
                path
            );

            let mut file = File::open(&path).await?;
            let mut contents = String::new();
            file.read_to_string(&mut contents).await?;

            let mut segment_operations = 0;
            let mut segment_errors = 0;

            for (line_num, line) in contents.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }

                match serde_json::from_str::<WalEntry>(line) {
                    Ok(entry) if entry.database == db_name => {
                        debug!(
                            "Replaying operation from line {}: {:?}",
                            line_num + 1,
                            entry.operation
                        );

                        match self
                            .replay_operation(&entry.operation, db_name, tinycache)
                            .await
                        {
                            Ok(_) => {
                                segment_operations += 1;
                                total_operations += 1;
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to replay operation from {}:{}: {}",
                                    path.display(),
                                    line_num + 1,
                                    e
                                );
                                segment_errors += 1;
                                skipped_operations += 1;
                            }
                        }
                    }
                    Ok(_) => {
                        debug!(
                            "Skipping operation for different database at line {}",
                            line_num + 1
                        );
                        continue;
                    }
                    Err(e) => {
                        error!(
                            "WAL parse error in {}:{}: {}",
                            path.display(),
                            line_num + 1,
                            e
                        );
                        segment_errors += 1;
                        skipped_operations += 1;
                        continue;
                    }
                }
            }

            info!(
                "Completed WAL segment {}/{}: {} operations replayed, {} errors",
                index + 1,
                wal_files.len(),
                segment_operations,
                segment_errors
            );
        }

        info!("WAL recovery completed for database '{}': {} total operations replayed, {} skipped due to errors", 
              db_name, total_operations, skipped_operations);
        Ok(())
    }

    /// Replays a single WAL operation during recovery.
    async fn replay_operation(
        &self,
        operation: &WalOperation,
        db_name: &str,
        tinycache: &TinyCache,
    ) -> io::Result<()> {
        match operation {
            WalOperation::Create { key, value, ttl } => {
                if let Some(ttl_duration) = ttl {
                    tinycache
                        .create_key_value_with_ttl(
                            db_name,
                            key.clone(),
                            value.clone(),
                            *ttl_duration,
                        )
                        .await?;
                } else {
                    tinycache
                        .create_key_value(db_name, key.clone(), value.clone())
                        .await?;
                }
            }
            WalOperation::Update { key, value, ttl } => {
                tinycache
                    .update_key_value(db_name, key, value.clone(), ttl.clone())
                    .await?;
            }
            WalOperation::Delete { key } => {
                tinycache.delete_key_value(db_name, key).await?;
            }
            WalOperation::Increment { key, amount } => {
                tinycache.increment_key_value(db_name, key, *amount).await?;
            }
            WalOperation::Decrement { key, amount } => {
                tinycache.decrement_key_value(db_name, key, *amount).await?;
            }
            WalOperation::DropDb => {
                tinycache.drop_db(db_name).await?;
            }
        }
        Ok(())
    }

    /// Cleans up old WAL segments based on wal_max_segments configuration.
    pub async fn cleanup_old_segments(&self, db_name: &str) -> io::Result<()> {
        if self.config.wal_max_segments == 0 {
            debug!(
                "WAL segment cleanup disabled (wal_max_segments = 0) for database '{}'",
                db_name
            );
            return Ok(());
        }

        debug!(
            "Checking for old WAL segments to cleanup for database '{}'",
            db_name
        );

        let wal_dir = &self.config.persist_dir;
        let mut read_dir = fs::read_dir(wal_dir).await?;
        let mut wal_files = Vec::new();

        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            if let Some(file_name) = path.file_name() {
                if let Some(name_str) = file_name.to_str() {
                    if name_str.starts_with(&format!("wal-{}-", db_name)) {
                        wal_files.push(path);
                    }
                }
            }
        }

        // Sort by filename (which includes timestamp)
        wal_files.sort();

        // Keep only the most recent segments
        let segments_to_remove = wal_files
            .len()
            .saturating_sub(self.config.wal_max_segments as usize);

        if segments_to_remove > 0 {
            info!(
                "Cleaning up {} old WAL segments for database '{}' (keeping {} most recent)",
                segments_to_remove, db_name, self.config.wal_max_segments
            );

            for path in wal_files.iter().take(segments_to_remove) {
                debug!("Removing old WAL segment: {:?}", path);
                if let Err(e) = fs::remove_file(path).await {
                    warn!("Failed to remove old WAL segment {:?}: {}", path, e);
                } else {
                    debug!("Successfully removed old WAL segment: {:?}", path);
                }
            }

            info!(
                "WAL cleanup completed for database '{}': {} segments removed",
                db_name, segments_to_remove
            );
        } else {
            debug!(
                "No WAL segments need cleanup for database '{}' ({} segments, limit: {})",
                db_name,
                wal_files.len(),
                self.config.wal_max_segments
            );
        }

        Ok(())
    }

    /// Recovers all databases found in the persist directory.
    pub async fn recover_all(&self, tinycache: &TinyCache) -> io::Result<()> {
        info!("Starting recovery for all databases");

        let wal_dir = &self.config.persist_dir;
        let mut read_dir = fs::read_dir(wal_dir).await?;
        let mut databases = std::collections::HashSet::new();

        // Discover all databases from WAL files
        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            if let Some(file_name) = path.file_name() {
                if let Some(name_str) = file_name.to_str() {
                    if name_str.starts_with("wal-") && name_str.ends_with(".log") {
                        // Extract database name from "wal-<db_name>-<timestamp>.log"
                        if let Some(db_part) = name_str.strip_prefix("wal-") {
                            if let Some(last_dash) = db_part.rfind('-') {
                                let db_name = &db_part[..last_dash];
                                databases.insert(db_name.to_string());
                            }
                        }
                    }
                }
            }
        }

        info!(
            "Discovered {} databases to recover: {:?}",
            databases.len(),
            databases
        );

        // Recover each database
        for db_name in databases {
            info!("Recovering database: '{}'", db_name);
            if let Err(e) = self.recover(&db_name, tinycache).await {
                error!("Failed to recover database '{}': {}", db_name, e);
            } else {
                info!("Successfully recovered database: '{}'", db_name);
            }
        }

        info!("Recovery completed for all databases");
        Ok(())
    }
}
