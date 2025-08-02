use crate::{
    db::cache::{Cache, CacheEntryType, CacheKey, CacheValue},
    persistance::persistance::{PersistenceConfig, PersistenceManager, WalOperation},
    security::{auth::AuthManager, config::DBConfig},
    utils::{
        logs::{LogLevel, Logger},
        utils::compute_expiry,
    },
};
use dashmap::DashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::{
    collections::HashMap,
    io::{self},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use tokio::sync::RwLock;

#[derive(Debug, PartialEq)]
pub enum DatabaseType {
    KeyValue,
}

impl DatabaseType {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "kv" => DatabaseType::KeyValue,
            _ => DatabaseType::KeyValue,
        }
    }
}

/// The DataValue struct represents the different types
// / of data structures that are supported by the database
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub enum DataValue {
    String(String),
    List(Vec<String>),
    Set(HashMap<String, ()>),
    Json(JsonValue),
}

/// Statistics for a single database instance.
#[derive(Debug, Serialize, Deserialize)]
pub struct DatabaseStats {
    pub entry_count: usize,      // Number of entries in the cache
    pub eviction_policy: String, // Current eviction policy
}

// This is the backborne of this server
// The TinyCache implementation lies in this struct
#[derive(Clone)]
pub struct TinyCache {
    pub databases: Arc<DashMap<String, Arc<tokio::sync::RwLock<Cache>>>>,
    pub auth_manager: Arc<AuthManager>, // *auth_manager* initialised, and tracks the database auth
    pub config: Arc<DBConfig>,          // *config* holds the database configurations
    pub logger: Logger,                 // *logger* is the internal logger for the system
    pub active_connections: Arc<RwLock<usize>>, // *active_connections* tracks the number of active connections
    pub current_database: Arc<RwLock<Option<String>>>, // *current_database* sets the current database
    pub persistence: Arc<PersistenceManager>,
}

impl TinyCache {
    /// *new* creates a new TinyCache instance
    /// It proceeds to initialise all the fields
    /// creates the database directory if it does not exists
    /// loads data from the disk if the database file already exists
    pub async fn new(
        data_dir: PathBuf,
        config: DBConfig,
        persist_config: PersistenceConfig,
    ) -> io::Result<Self> {
        let auth_manager = AuthManager::new(config.clone());
        let logger = Logger::new(data_dir.clone()).await?;
        let persistence = PersistenceManager::new(persist_config).await?;

        let tinycache = TinyCache {
            databases: Arc::new(DashMap::new()),
            auth_manager: Arc::new(auth_manager),
            config: Arc::new(config.clone()),
            logger,
            active_connections: Arc::new(RwLock::new(0)),
            current_database: Arc::new(RwLock::new(None)),
            persistence: Arc::new(persistence),
        };

        tinycache.ensure_db_exists("default").await;

        tinycache.recover_all().await?;

        Ok(tinycache)
    }

    pub async fn recover_all(&self) -> io::Result<()> {
        self.logger
            .log_info(
                "Initiating database recovery through persistence manager",
                LogLevel::System,
                &self,
            )
            .await?;
        self.persistence.recover_all(self).await
    }

    ////////////////////////////////////////////////////////////////////////////////////////////
    ///////////////////////////////////// DATABASE and WAL /////////////////////////////////////
    ////////////////////////////////////////////////////////////////////////////////////////////

    /// *set_current_database* sets the current database
    pub async fn set_current_database(&self, database: Option<&str>) {
        let mut current = self.current_database.write().await;
        *current = database.map(String::from);
    }

    /// *ensure_db_exists* ensures a database exists in memory and initialised
    async fn ensure_db_exists(&self, database: &str) {
        self.databases
            .entry(database.to_string())
            .or_insert_with(|| {
                Arc::new(tokio::sync::RwLock::new(Cache::new(
                    self.config.max_entries,
                    self.config.eviction_policy.clone(),
                    self.config.worker_threads,
                )))
            });
    }

    /// Retrieves statistics for a specific database.
    ///
    /// # Arguments
    /// * `database` - The name of the database to analyze.
    ///
    /// # Returns
    /// A `DatabaseStats` struct with entry count, estimated memory usage, and other metrics.
    ///
    pub async fn get_database_stats(&self, database: &str) -> Option<DatabaseStats> {
        let cache = match self.databases.get(database) {
            Some(cache_ref) => cache_ref.clone(),
            None => return None,
        };

        let cache_lock = cache.read().await;
        let mut entry_count = 0;
        for shard in cache_lock.shards.iter() {
            entry_count += shard.read().await.len();
        }

        Some(DatabaseStats {
            entry_count,
            eviction_policy: cache_lock.eviction_policy.clone(),
        })
    }

    /// Retrieves statistics for all databases in the TinyCache instance.
    ///
    /// # Returns
    /// A HashMap mapping database names to their `DatabaseStats`.
    pub async fn get_all_database_stats(&self) -> HashMap<String, DatabaseStats> {
        let mut stats = HashMap::new();
        for entry in self.databases.iter() {
            let db_name = entry.key().clone(); // Clone the database name
            let cache = entry.value(); // Get the Arc<RwLock<Cache>>
            let cache_lock = cache.read().await;

            let mut entry_count = 0;
            for shard in cache_lock.shards.iter() {
                entry_count += shard.read().await.len(); // Async lock each shard
            }

            stats.insert(
                db_name,
                DatabaseStats {
                    entry_count,
                    eviction_policy: cache_lock.eviction_policy.clone(),
                },
            );
        }
        stats
    }

    /// *drop_db* clears all database files
    pub async fn drop_db(&self, database: &str) -> io::Result<()> {
        self.persistence
            .log_operation(database, WalOperation::DropDb)
            .await?;

        let removed = self.databases.remove(database);
        if let Some((_, cache)) = removed {
            let mut cache_lock = cache.write().await;

            for shard in cache_lock.shards.iter_mut() {
                shard.write().await.clear();
            }

            for queue in cache_lock.lru_queues.iter_mut() {
                queue.write().await.clear();
            }
        }

        Ok(())
    }

    /// *get_cache* is a helper function which is usefull for getting the RwLock for a cache
    ///
    /// This cache is associated to a specific database, hense the database field
    pub async fn get_cache(&self, database: &str) -> Arc<RwLock<Cache>> {
        self.ensure_db_exists(database).await;
        self.databases.get(database).unwrap().clone()
    }

    ////////////////////////////////////////////////////////////////////////////////////////////
    /////////////////////////////////////// KEY_VALUE //////////////////////////////////////////
    ////////////////////////////////////////////////////////////////////////////////////////////

    /// *create_key_value* sets a key-value pair in the in-memory database
    /// updates indexes for JSON format
    /// updates the Least-Recently-Used cache
    /// saves changes to disk
    pub async fn create_key_value(
        &self,
        database: &str,
        key: String,
        value: DataValue,
    ) -> io::Result<()> {
        // Log the operation to WAL first (for durability)
        self.persistence
            .log_operation(
                database,
                WalOperation::Create {
                    key: key.clone(),
                    value: value.clone(),
                    ttl: if self.config.default_ttl_secs > 0 {
                        Some(Duration::from_secs(self.config.default_ttl_secs))
                    } else {
                        compute_expiry()
                    },
                },
            )
            .await?;

        // Then update the in-memory cache
        let cache = self.get_cache(database).await;
        let expiry = if self.config.default_ttl_secs > 0 {
            Some(Duration::from_secs(self.config.default_ttl_secs))
        } else {
            compute_expiry()
        };

        cache
            .write()
            .await
            .insert_key_value(database, key, value, expiry)
            .await;
        Ok(())
    }

    pub async fn create_key_value_with_ttl(
        &self,
        database: &str,
        key: String,
        value: DataValue,
        ttl: Duration,
    ) -> io::Result<()> {
        self.persistence
            .log_operation(
                database,
                WalOperation::Create {
                    key: key.clone(),
                    value: value.clone(),
                    ttl: Some(ttl),
                },
            )
            .await?;

        let cache = self.get_cache(database).await;
        let mut cache_lock = cache.write().await;

        cache_lock
            .insert_key_value(database, key, value, Some(ttl))
            .await;
        Ok(())
    }

    pub async fn get_key_value(&self, database: &str, key: &str) -> Option<DataValue> {
        let cache = self.get_cache(database).await;
        let mut cache_lock = cache.write().await;
        cache_lock.get_key_value(database, key).await
    }

    pub async fn delete_key_value(&self, database: &str, key: &str) -> io::Result<bool> {
        self.persistence
            .log_operation(
                database,
                WalOperation::Delete {
                    key: key.to_string(),
                },
            )
            .await?;

        let cache = self.get_cache(database).await;
        let mut cache_lock = cache.write().await;
        let cache_key = CacheKey {
            database: database.to_string(),
            key: key.to_string(),
            entry_type: CacheEntryType::KeyValue,
        };
        let deleted = cache_lock.delete(&cache_key).await.is_some();

        Ok(deleted)
    }

    pub async fn increment_key_value(
        &self,
        database: &str,
        key: &str,
        amount: f64,
    ) -> io::Result<Option<f64>> {
        self.persistence
            .log_operation(
                database,
                WalOperation::Increment {
                    key: key.to_string(),
                    amount,
                },
            )
            .await?;

        let cache = self.get_cache(database).await;
        let mut cache_lock = cache.write().await;

        let result = cache_lock.incr_key_value(database, key, amount).await;

        Ok(result)
    }

    pub async fn decrement_key_value(
        &self,
        database: &str,
        key: &str,
        amount: f64,
    ) -> io::Result<Option<f64>> {
        self.persistence
            .log_operation(
                database,
                WalOperation::Decrement {
                    key: key.to_string(),
                    amount,
                },
            )
            .await?;
        self.increment_key_value(database, key, -amount).await // Reuse increment with negative amount
    }

    pub async fn update_key_value(
        &self,
        database: &str,
        key: &str,
        value: DataValue,
        ttl: Option<Duration>,
    ) -> io::Result<Option<DataValue>> {
        self.persistence
            .log_operation(
                database,
                WalOperation::Update {
                    key: key.to_string(),
                    value: value.clone(),
                    ttl,
                },
            )
            .await?;
        let cache = self.get_cache(database).await;
        let old_value = cache
            .write()
            .await
            .update_key_value(database, key, value.clone(), ttl)
            .await;
        Ok(old_value)
    }

    // Returns a Json representation of all data in the database
    // Includes key, value, and expiry information
    pub async fn view_data(&self, database: &str) -> JsonValue {
        let cache = self.get_cache(database).await;
        let cache_lock = cache.read().await;
        let mut data_map = serde_json::Map::new();
        for shard in cache_lock.shards.iter() {
            let shard_lock = shard.read().await;
            for (cache_key, item) in shard_lock.iter() {
                if cache_key.database != database
                    || cache_key.entry_type != CacheEntryType::KeyValue
                {
                    continue;
                }
                let CacheValue::KeyValue(value, expiry) = &item.value;
                let value_data = match value {
                    DataValue::String(s) => {
                        json!({"type": "String", "value": s, "expiry": expiry, "created_at": item.created_at})
                    }
                    DataValue::List(l) => json!({"type": "List", "value": l, "expiry": expiry}),
                    DataValue::Set(s) => {
                        json!({"type": "Set", "value": s.keys().collect::<Vec<_>>(), "expiry": expiry})
                    }
                    DataValue::Json(j) => {
                        json!({"type": "Json", "value": j, "expiry": expiry, "created_at": item.created_at})
                    }
                };
                data_map.insert(cache_key.key.clone(), value_data);
            }
        }
        json!(data_map)
    }
}
