use serde_json::Value as JsonValue;
use std::{
    collections::{self, HashMap, VecDeque},
    hash::{Hash, Hasher},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::{
    constants::constants::{LFRU, LFU, LRU},
    db::db::DataValue,
    utils::utils::{compute_expiry_using_ttl, compute_now_timestamp},
};

#[derive(Clone, Hash, Eq, PartialEq)]
/// Represents a key used for caching purposes.
///
/// The `CacheKey` struct is used to uniquely identify a cached item
/// within a specific collection.
pub struct CacheKey {
    pub database: String, // * `database` - The name of the database to which the cached item belongs.
    pub key: String, // * `key` - The unique key identifying the cached item within the database.
    pub entry_type: CacheEntryType,
}

#[derive(Clone, Hash, Eq, PartialEq, Debug, Serialize, Deserialize, Copy)]
pub enum CacheEntryType {
    KeyValue,
}

#[derive(Clone)]
pub enum CacheValue {
    KeyValue(DataValue, Option<u64>), // Value and expiry
}

pub struct CacheItem {
    pub value: CacheValue,   // the value that is stored in cache
    pub created_at: u64,     // creation timestamp
    pub expiry: Option<u64>, // expiry time of the value in cache
    pub last_access: u64,    // when lasest was the value accessed
    pub frequency: u32,      // the frequency in which that value is accessed
}

// LFRUCache implementation

/// LFRUCache is going to be useful for caching data (keeping it in memory).
/// It combines Least Frequently Used and Least Recently Used algorithms to
/// optimize memory usage
/// it ensures that most relevant data is kept in memory for quick access.
/// Least frequently used data will be removed from memory, as well as least recently used data
pub struct Cache {
    pub shards: Vec<RwLock<HashMap<CacheKey, CacheItem>>>,
    pub lru_queues: Vec<RwLock<VecDeque<CacheKey>>>,
    pub shard_count: usize, // total shard count
    pub metrics: Arc<CacheMetrics>,
    pub max_size: usize, // we are using this to set the max_size of the cache (entries)
    pub frequency_threshold: u32, // per item, we are going to use this requency threshold to determine if we keep it in Cache or not
    pub time_threshold: Duration, // the is the time version of the frequency_threshold
    pub eviction_policy: String,  // eviction policy for items in cache
}

pub struct CacheMetrics {
    hits: AtomicU64,
    misses: AtomicU64,
    shard_loads: Vec<AtomicU64>, // load per shard
}

/// implementation of the LFRUCache
/// we have functions
/// 1. new which initializes everything
/// 2. insert which will be used primarily to store data itself in memory
/// 3. get which will be used to retrieve the data itself.
/// 4. evict for eviction policy based on usage and expiration time
impl Cache {
    /// Creates a new Cache instance with the specified maximum size and eviction policy.
    ///
    /// # Arguments
    /// * `max_size` - Maximum number of entries the cache can hold.
    /// * `eviction_policy` - The eviction strategy: "LFRU" (hybrid), "LRU" (recently used), or "LFU" (frequently used).
    ///
    /// # Returns
    /// A new `Cache` instance.
    pub fn new(max_size: usize, eviction_policy: String, shard_count: usize) -> Self {
        let shard_count = shard_count.next_power_of_two(); // ensures shard count is a power of 2
        let shard_size = max_size / shard_count;
        let mut shards = Vec::with_capacity(shard_count);
        let mut lru_queues = Vec::with_capacity(shard_count);
        for _ in 0..shard_count {
            shards.push(RwLock::new(HashMap::with_capacity(shard_size)));
            lru_queues.push(RwLock::new(VecDeque::new()));
        }

        let metrics = Arc::new(CacheMetrics::new(shard_count));

        Cache {
            shards,
            lru_queues,
            shard_count: shard_count.max(1),
            metrics,
            max_size,
            frequency_threshold: 5,
            time_threshold: Duration::from_secs(3600),
            eviction_policy,
        }
    }

    pub fn get_shard_index(&self, key: &CacheKey) -> usize {
        let mut hasher = collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        let hash = hasher.finish() as usize;

        // using bitwise AND for power-of-2 shared_count
        hash & (self.shard_count - 1)
    }

    // Optional: Load-aware shard selection for inserts
    pub async fn _get_balanced_shard_index(&self, key: &CacheKey) -> usize {
        let base_idx = self.get_shard_index(key);
        let shard_size = self.shards[base_idx].read().await.len();
        let threshold = self.max_size / self.shard_count;

        // If the base shard is below threshold, use it
        if shard_size < threshold {
            return base_idx;
        }

        // Otherwise, find the least loaded shard
        let mut min_load = shard_size;
        let mut min_idx = base_idx;
        for i in 0..self.shard_count {
            let load = self.shards[i].read().await.len();
            if load < min_load {
                min_load = load;
                min_idx = i;
            }
        }
        min_idx
    }

    // pub async fn log_metrics(&self) {
    //     let hit_rate = self.metrics.hit_rate();
    //     println!("Cache Hit Rate: {:.2}%", hit_rate);
    //     for i in 0..self.shard_count {
    //         let load = self.metrics.shard_load(i);
    //         println!("Shard {} Load: {}", i, load);
    //     }
    // }

    ////////////////////////////////////////////////////////////////////////////////////////////
    /////////////////////////////////////// COMMON /////////////////////////////////////////////
    ////////////////////////////////////////////////////////////////////////////////////////////

    // share insert logic for inserting data into the database cache
    pub async fn insert(&mut self, key: CacheKey, value: CacheValue, ttl: Option<Duration>) {
        let shard_idx = self.get_shard_index(&key);
        let mut shard = self.shards[shard_idx].write().await;
        let mut lru_queue = self.lru_queues[shard_idx].write().await;

        if shard.len() >= self.max_size / self.shard_count {
            drop(shard);
            drop(lru_queue);
            self.evict_shard(shard_idx).await; // Asynchronous eviction
            shard = self.shards[shard_idx].write().await;
            lru_queue = self.lru_queues[shard_idx].write().await;
        }

        let now = compute_now_timestamp();

        let expiry = compute_expiry_using_ttl(ttl);

        let item = CacheItem {
            value,
            created_at: now,
            frequency: 1,
            last_access: now,
            expiry,
        };

        shard.insert(key.clone(), item);

        // to ensure that the most recently added item is at the end of the queue,
        // we need to push the item to the back of the queue
        lru_queue.push_back(key);
    }

    // TODO: update the ttl since it has been updated
    pub async fn get(&self, key: &CacheKey) -> Option<CacheValue> {
        let shard_idx = self.get_shard_index(key);
        let mut shard = self.shards[shard_idx].write().await;

        let mut lru_queue = self.lru_queues[shard_idx].write().await;

        let now = compute_now_timestamp();

        self.metrics
            .update_shard_load(shard_idx, shard.len() as u64);

        if let Some(item) = shard.get_mut(key) {
            if item.expiry.map_or(false, |e| now > e) {
                shard.remove(key);
                lru_queue.retain(|k| k != key);
                self.metrics.record_miss();
                return None;
            }
            item.frequency += 1;
            item.last_access = now;

            lru_queue.retain(|k| k != key);
            lru_queue.push_back(key.clone());
            self.metrics.record_hit();
            Some(item.value.clone())
        } else {
            self.metrics.record_miss();
            None
        }
    }

    pub async fn delete(&mut self, key: &CacheKey) -> Option<CacheValue> {
        let shard_idx = self.get_shard_index(key);
        let mut shard = self.shards[shard_idx].write().await;
        let mut lru_queue = self.lru_queues[shard_idx].write().await;

        if let Some(item) = shard.remove(key) {
            lru_queue.retain(|k| k != key);
            Some(item.value)
        } else {
            None
        }
    }

    async fn evict_shard(&mut self, shard_idx: usize) {
        let mut shard = self.shards[shard_idx].write().await;
        let mut lru_queue = self.lru_queues[shard_idx].write().await;

        let now = compute_now_timestamp();

        // Remove expired items
        let expired: Vec<CacheKey> = shard
            .iter()
            .filter_map(|(k, v)| {
                v.expiry
                    .and_then(|e| if now > e { Some(k.clone()) } else { None })
            })
            .collect();

        for key in expired {
            shard.remove(&key);
            lru_queue.retain(|k| k != &key);
        }

        // eviction based on policy
        while shard.len() >= self.max_size / self.shard_count {
            match self.eviction_policy.as_str() {
                LRU => {
                    if let Some(key) = lru_queue.pop_front() {
                        shard.remove(&key);
                        lru_queue.retain(|k| k != &key);
                    } else {
                        break;
                    }
                }

                LFU => {
                    if let Some((key, _)) = shard.iter().min_by_key(|(_, item)| item.frequency) {
                        let key = key.clone();
                        shard.remove(&key);
                        lru_queue.retain(|k| k != &key);
                    }
                }

                LFRU => {
                    let mut candidates: Vec<(CacheKey, u32, u64)> = shard
                        .iter()
                        .map(|(k, v)| (k.clone(), v.frequency, v.last_access))
                        .collect();

                    // Sort by combined score: frequency (ascending) + age penalty (descending by last_access)
                    candidates.sort_by(|a, b| {
                        let freq_cmp = a.1.cmp(&b.1);
                        if freq_cmp == std::cmp::Ordering::Equal {
                            b.2.cmp(&a.2)
                        } else {
                            freq_cmp
                        }
                    });

                    let time_threshold_secs = self.time_threshold.as_secs();
                    let mut evicted = false;

                    for (key, frequency, last_access) in candidates.clone() {
                        let age = now.saturating_sub(last_access);

                        // Evict if item is both below frequency threshold AND older than time threshold
                        if frequency < self.frequency_threshold && age > time_threshold_secs {
                            shard.remove(&key);
                            lru_queue.retain(|k| k != &key);
                            evicted = true;
                            break;
                        }
                    }
                    // If no items met both thresholds, fall back to the least recently used item
                    // that has low frequency (below threshold)
                    if !evicted {
                        for (key, frequency, _) in &candidates {
                            if frequency < &self.frequency_threshold {
                                shard.remove(key);
                                lru_queue.retain(|k| k != key);
                                evicted = true;
                                break;
                            }
                        }
                    }

                    // Final fallback: evict the oldest item regardless of frequency
                    if !evicted {
                        if let Some((key, _, _)) = candidates.first() {
                            shard.remove(key);
                            lru_queue.retain(|k| k != key);
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
    }

    ////////////////////////////////////////////////////////////////////////////////////////////
    /////////////////////////////////////// KEY_VALUE //////////////////////////////////////////
    ////////////////////////////////////////////////////////////////////////////////////////////

    pub async fn insert_key_value(
        &mut self,
        database: &str,
        key: String,
        value: DataValue,
        ttl: Option<Duration>,
    ) {
        let cache_key = CacheKey {
            database: database.to_string(),
            key,
            entry_type: CacheEntryType::KeyValue,
        };

        self.insert(
            cache_key,
            CacheValue::KeyValue(value, ttl.map(|d| d.as_secs())),
            ttl,
        )
        .await;
    }

    pub async fn get_key_value(&mut self, database: &str, key: &str) -> Option<DataValue> {
        let cache_key = CacheKey {
            database: database.to_string(),
            key: key.to_string(),
            entry_type: CacheEntryType::KeyValue,
        };

        self.get(&cache_key).await.and_then(|v| match v {
            CacheValue::KeyValue(val, _) => Some(val),
        })
    }

    pub async fn update_key_value(
        &mut self,
        database: &str,
        key: &str,
        value: DataValue,
        ttl: Option<Duration>,
    ) -> Option<DataValue> {
        let cache_key = CacheKey {
            database: database.to_string(),
            key: key.to_string(),
            entry_type: CacheEntryType::KeyValue,
        };

        let shard_idx = self.get_shard_index(&cache_key);
        let mut shard = self.shards[shard_idx].write().await;
        let mut lru_queue = self.lru_queues[shard_idx].write().await;

        if let Some(item) = shard.get_mut(&cache_key) {
            let old_value = match &item.value {
                CacheValue::KeyValue(v, _) => v.clone(),
            };

            let now = compute_now_timestamp();

            let expiry = compute_expiry_using_ttl(ttl);

            item.value = CacheValue::KeyValue(value, ttl.map(|d| d.as_secs()));
            item.expiry = expiry;
            item.frequency += 1;
            item.last_access = now;

            lru_queue.retain(|k| k != &cache_key);
            lru_queue.push_back(cache_key);

            Some(old_value)
        } else {
            None
        }
    }

    pub async fn incr_key_value(&mut self, database: &str, key: &str, amount: f64) -> Option<f64> {
        let cache_key = CacheKey {
            database: database.to_string(),
            key: key.to_string(),
            entry_type: CacheEntryType::KeyValue,
        };
        let shard_idx = self.get_shard_index(&cache_key);
        let mut shard = self.shards[shard_idx].write().await;
        let mut lru_queue = self.lru_queues[shard_idx].write().await;

        if let Some(item) = shard.get_mut(&cache_key) {
            let now = compute_now_timestamp();

            let CacheValue::KeyValue(ref mut value, _expiry) = item.value;
            let current = match value {
                DataValue::String(s) => s.parse::<f64>().ok(),
                DataValue::Json(j) => j.as_f64(),
                _ => None,
            };
            if let Some(num) = current {
                let new_value = num + amount;
                *value = DataValue::Json(JsonValue::Number(
                    serde_json::Number::from_f64(new_value).unwrap_or(serde_json::Number::from(0)),
                ));
                item.frequency += 1;
                item.last_access = now;

                lru_queue.retain(|k| k != &cache_key);
                lru_queue.push_back(cache_key);
                return Some(new_value);
            }
        }
        None
    }

    // pub fn _decr_key_value(&mut self, database: &str, key: &str, amount: f64) -> Option<f64> {
    //     self.incr_key_value(database, key, -amount).await
    // }

    async fn _remove(&mut self, key: &CacheKey) {
        let shard_idx = self.get_shard_index(key);
        let mut lru_queue = self.lru_queues[shard_idx].write().await;

        lru_queue.retain(|k| k != key);
    }
}

impl CacheMetrics {
    // creates new metrics for a given shard count
    pub fn new(shard_count: usize) -> Self {
        let mut shard_loads = Vec::with_capacity(shard_count);
        for _ in 0..shard_count {
            shard_loads.push(AtomicU64::new(0));
        }
        CacheMetrics {
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            shard_loads,
        }
    }

    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the miss counter.
    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Updates the load for a shard.
    pub fn update_shard_load(&self, shard_idx: usize, load: u64) {
        self.shard_loads[shard_idx].store(load, Ordering::Relaxed);
    }

    /// Returns the hit rate as a percentage.
    pub fn _hit_rate(&self) -> f64 {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            (hits as f64 / total as f64) * 100.0
        }
    }

    /// Returns the load of a specific shard.
    pub fn _shard_load(&self, shard_idx: usize) -> u64 {
        self.shard_loads[shard_idx].load(Ordering::Relaxed)
    }
}
