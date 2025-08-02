Using shards
shards: Vec<HashMap<CacheKey, CacheItem>>
[ Shard 0 ] [ Shard 1 ] [ Shard 2 ]
|------------| |------------| |------------|
| Key1 -> Item1| | Key2 -> Item2| | Key3 -> Item3|
|------------| |------------| |------------|

lru_queues: Vec<VecDeque<CacheKey>>
[ Shard 0 ] [ Shard 1 ] [ Shard 2 ]
|Key1, ...| |Key2, ...| |Key3, ...|

TinyCache Instance
├── Workspace: "123"
│ ├── Database: "myapp:kv"
│ │ └── Cache: { max_size: 10000, eviction_policy: "LRU" }
│ │ ├── "user:1" → "Alice"
│ │ ├── "counter" → 42
│ │ └── "settings" → {"theme": "dark", "notifications": true}
│ ├── Database: "myapp:docs"
│ │ └── Cache: { max_size: 5000, eviction_policy: "LFRU" }
│ │ ├── "doc:abc123" → {"\_id": "abc123", "title": "Order #1", "amount": 100}
│ │ └── "doc:xyz789" → {"\_id": "xyz789", "title": "Order #2", "amount": 50}
│ ├── Database: "myapp:pubsub"
│ │ └── Cache: { max_size: 2000, eviction_policy: "LFU", max_channel_subs: 1000 }
│ │ ├── Channel: "order_updates"
│ │ │ ├── Subscribers: [Client1, Client2]
│ │ │ └── Last Message: {"order_id": "o1", "status": "shipped"}
│ │ └── Channel: "user_notifications"
│ │ ├── Subscribers: [Client3]
│ │ └── Last Message: {"user_id": "u1", "message": "Welcome!"}
│ ├── Database: "myapp:queues"
│ │ └── Cache: { max_size: 3000, eviction_policy: "LRU", max_queue_size: 5000 }
│ │ ├── Queue: "payment_tasks"
│ │ │ └── Messages: [{"task_id": "t1", "amount": 100}, {"task_id": "t2", "amount": 200}]
│ │ └── Queue: "email_tasks"
│ │ └── Messages: [{"email": "alice@example.com", "subject": "Order Confirmation"}]
│ ├── Database: "myapp:streams"
│ │ └── Cache: { max_size: 4000, eviction_policy: "LFRU", max_stream_size: 10000 }
│ │ ├── Stream: "transactions"
│ │ │ └── Events:
│ │ │ ├── {"id": "123-456", "data": {"order_id": "o1", "amount": 100}, "timestamp": 1234567890}
│ │ │ └── {"id": "123-457", "data": {"order_id": "o2", "amount": 50}, "timestamp": 1234567891}
│ │ └── Stream: "user_actions"
│ │ └── Events:
│ │ └── {"id": "123-458", "data": {"user_id": "u1", "action": "login"}, "timestamp": 1234567892}
│ └── Database: "myapp:hybrid" (Optional)
│ └── Cache: { max_size: 15000, eviction_policy: "LFRU" }
│ ├── Key-Value Store
│ ├── Document Store
│ ├── Pub/Sub
│ ├── Message Queues
│ └── Event Streams

let (max_size, max_stream_size, max_queue_size, max_channel_subs) = match allowed_type {
CacheEntryType::KeyValue => (10000, 0, 0, 0),
CacheEntryType::Document => (5000, 0, 0, 0),
CacheEntryType::PubSubChannel => (2000, 0, 0, 1000),
CacheEntryType::EventStream => (4000, 10000, 0, 0),
CacheEntryType::MessageQueue => (3000, 0, 5000, 0),
CacheEntryType::Vector => (4000, 0, 0, 0),
CacheEntryType::Hybrid => (15000, 1000, 1000, 1000), // Hybrid allows all
};
