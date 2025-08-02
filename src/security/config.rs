/// config.rs is the main database configuration fle.
/// if contains configurations for the memory database.
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
};

use tokio::{
    fs,
    io::{self, AsyncWriteExt},
};

use crate::constants::constants::{DEFAULT_PORT, KEY_VALUE, LFRU, LFU, LRU, CONFIG_FILE};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DBConfig {
    // authentication settings
    pub admin: String,
    pub password: String,
    pub database: String,
    pub database_type: String,
    pub salt: String,
    pub session_ttl: u64,

    // server configurations
    pub host: IpAddr,           // IP address the database server listens on
    pub port: String,           // Network port for database comminication
    pub max_connections: usize, // Maximum concurrent client connections

    // memory database settings
    pub max_entries: usize,    // Maximum number of entries per database cache
    pub default_ttl_secs: u64, // Default TTL for entries (0 = no expiry)
    pub checkpoint_interval_secs: u64, // Interval for periodic checkpoints

    // Performance tuning
    pub worker_threads: usize, // Number of worker threads for parallel processing for optimized CPU utilization
    pub eviction_policy: String, // "LFRU", "LFU", or "LFU"
}

impl Default for DBConfig {
    fn default() -> Self {
        Self {
            admin: String::new(),
            password: String::new(),
            database: String::new(),
            database_type: KEY_VALUE.to_string(),
            salt: String::new(),
            session_ttl: 86400,

            host: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            port: DEFAULT_PORT.to_string(),
            max_connections: 20,

            max_entries: 1200,
            default_ttl_secs: 604800,
            checkpoint_interval_secs: 3600,

            worker_threads: num_cpus::get(),
            eviction_policy: LFRU.to_string(),
        }
    }
}

impl DBConfig {
    /// load_or_create is used to load the configuration if the database is already configured, else create a new one
    ///
    /// this is achievable by simple checking if the configuration file exisits or not
    /// if database configuration does not exists, there default database is set.
    pub async fn load_or_create(data_dir: &PathBuf) -> io::Result<Self> {
        let config_path = data_dir.join(CONFIG_FILE);

        if config_path.exists() {
            let content = fs::read_to_string(&config_path).await?;
            let config: Self = toml::from_str(&content).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("failed to and convert config file: {}", e),
                )
            })?;

            Ok(config)
        } else {
            let config = Self::default();
            config.save(data_dir).await?;
            Ok(config)
        }
    }

    /// *save* saves the newly configured database to the configuration file
    ///
    pub async fn save(&self, data_dir: &PathBuf) -> io::Result<()> {
        let config_path = data_dir.join(CONFIG_FILE);

        let content = toml::to_string_pretty(&self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            // .mode(0o400)
            .open(&config_path)
            .await?;

        file.write_all(content.as_bytes()).await?;
        file.sync_all().await?;

        Ok(())
    }

    /// validation will be usefull in validating and making sure configurations are
    /// set to what they are suppose to be
    pub fn validate(&self) -> Result<(), String> {
        if self.worker_threads == 0 || self.worker_threads > num_cpus::get() * 2 {
            return Err(format!(
                "invalid worker thread count. Must be between 1 and {}",
                num_cpus::get() * 2
            ));
        }

        if self.max_entries < 100 {
            return Err("max_entries must be at least 100".to_string());
        }

        if ![LFRU, LRU, LFU].contains(&self.eviction_policy.as_str()) {
            return Err("eviction_policy must be 'LFRU', 'LRU', or 'LFU'".to_string());
        }

        Ok(())
    }
}
