TinyCache (Root Instance)
├── databases (Arc<RwLock<HashMap<String, Arc<RwLock<Cache>>>>>)
│ ├── "default" (Database Name)
│ │ └── Cache (In-memory storage for this database)
│ │ ├── items (HashMap<CacheKey, CacheItem>)
│ │ │ ├── KeyValue Entries
│ │ │ │ ├── CacheKey { database: "default", key: "user1", entry_type: KeyValue }
│ │ │ │ │ └── CacheItem { value: KeyValue(DataValue::Json({"name": "Alice"}), None), ... }
│ │ │ │ └── CacheKey { database: "default", key: "score", entry_type: KeyValue }
│ │ │ │ └── CacheItem { value: KeyValue(DataValue::String("100"), Some(3600)), ... }
│ │ │ ├── PubSubChannel Entries
│ │ │ │ ├── CacheKey { database: "default", key: "chatroom", entry_type: PubSubChannel }
│ │ │ │ │ └── CacheItem { value: PubSubChannel(HashSet<Sender<String>>, "session123"), ... }
│ │ │ │ └── CacheKey { database: "default", key: "alerts", entry_type: PubSubChannel }
│ │ │ │ └── CacheItem { value: PubSubChannel(HashSet<Sender<String>>, "session456"), ... }
│ │ │ ├── EventStream Entries
│ │ │ │ ├── CacheKey { database: "default", key: "user_events", entry_type: EventStream }
│ │ │ │ │ └── CacheItem { value: EventStream(Vec<StreamEntry>[{id: "123-1", data: {"event": "login"}, timestamp: 169...}, ...], "session789"), ... }
│ │ │ │ └── CacheKey { database: "default", key: "system_logs", entry_type: EventStream }
│ │ │ │ └── CacheItem { value: EventStream(Vec<StreamEntry>[...], "session101"), ... }
│ │ │ ├── MessageQueue Entries
│ │ │ │ ├── CacheKey { database: "default", key: "task_queue", entry_type: MessageQueue }
│ │ │ │ │ └── CacheItem { value: MessageQueue(VecDeque<JsonValue>[{"task": "process"}, {"task": "send_email"}], "session111"), ... }
│ │ │ │ └── CacheKey { database: "default", key: "job_queue", entry_type: MessageQueue }
│ │ │ │ └── CacheItem { value: MessageQueue(VecDeque<JsonValue>[...], "session222"), ... }
│ │ └── lru_queue (VecDeque<CacheKey>) [Tracks eviction order]
│ ├── "myapp" (Another Database Name)
│ │ └── Cache (Similar structure as above)
│ └── ... (Other databases)
├── wal_writer (Persists all changes to disk)
└── ... (Other TinyCache fields like auth_manager, config, etc.)

Explanation:
Top Level (TinyCache): The root object holds a collection of databases in a HashMap, each mapped to a Cache instance.
Database Level: Each database (e.g., "default", "myapp") has its own Cache, which manages all data types.
Cache Level:
KeyValue: Traditional key-value pairs (e.g., strings, JSON) with optional TTLs.
PubSubChannel: Channels with a set of subscribers (Sender<String>) and a creator ID, used for real-time messaging.
EventStream: Ordered lists of events (StreamEntry) with IDs, timestamps, and creator metadata, suitable for logging or replay.
MessageQueue: FIFO queues (VecDeque) of messages with creator metadata, used for task processing.
LRU Queue: Tracks the order of access for eviction across all entry types in the Cache.
WAL: Ensures durability by logging all operations (sets, deletes, etc.) across all data types.
Usage Scenario:
Key-Value: Store user data ("user1" -> {"name": "Alice"}).
Pub/Sub: Broadcast chat messages ("chatroom" -> "Hello!" to all subscribers).
Event Streams: Log user actions ("user_events" -> [{"id": "123-1", "event": "login"}, ...]).
Message Queues: Queue tasks ("task_queue" -> [{"task": "process"}, ...]).
This hierarchy allows a user to leverage all services within a single database context, with data segregated by type and key.
s

This is a modern, in-memory database with a focus on performance and concurrency. It uses advanced caching techniques and supports multiple data types while maintaining thread safety and providing monitoring capabilities. The architecture is designed to be scalable and can operate in both standalone and cloud environments.

Server Architecture:
TCP-based server implementation
Asynchronous I/O using Tokio runtime
Handles multiple client connections concurrently
Each client connection is handled in a separate task

Multiple shards for concurrent access

Multiple shards for concurrent access

let to_remove: Vec<CacheKey> = lru_queue
.iter()
.filter(|k| {
let item = &shard[k];
item.frequency < self.frequency_threshold
// || item.last_access > self.time_threshold // TODO: fix here
})
.cloned()
.collect();
for key in to_remove {
shard.remove(&key);
lru_queue.retain(|k| k != &key);
}
if shard.len() >= self.max_size / self.shard_count && !lru_queue.is_empty() {
let key = lru_queue.pop_front().unwrap();
shard.remove(&key);
}
