# TinyCache

**A High-Performance, Multi-Model In-Memory Database in Rust**

TinyCache is an innovative, high-performance, multi-model in-memory database engine built from the ground up in Rust. Leveraging Rust's memory safety, zero-cost abstractions, and concurrency primitives, TinyCache delivers a scalable, thread-safe solution for key-value, document, messaging, and vector data models.

## 🚀 Why TinyCache?

TinyCache addresses the need for databases that balance speed, flexibility, and reliability. It bridges the gap between traditional databases and in-memory solutions by offering:

- **Performance**: Sub-millisecond latency with sharded parallelism
- **Flexibility**: Multi-model support (key-value, document, messaging, vector)
- **Safety**: Rust's compile-time guarantees eliminate data races and memory bugs
- **Scalability**: Configurable worker threads and fine-grained locking for multi-core utilization
- **Concurrency**: Asynchronous operations with minimal contention

## Getting Started

### Quick Start Commands

#### Using Makefile
```bash
# Build and run TinyCache (release mode)
make run

# Build only (release mode)
make build

# Default target (runs the application)
make
```

#### Direct Rust Commands
```bash
# Run in development mode
cargo run

# Run in release mode (optimized)
cargo run --release

# Build in development mode
cargo build

# Build in release mode (optimized)
cargo build --release

# Run tests
cargo test

# Check code without building
cargo check

# Format code
cargo fmt

# Run clippy lints
cargo clippy
```

### Running the Server

Once built and running, TinyCache starts a TCP server that accepts client connections. The server will:

1. Initialize the in-memory database with default configurations
2. Start the WAL (Write-Ahead Logging) system for persistence
3. Begin accepting client connections on the configured port
4. Start background tasks for metrics logging and maintenance

### Basic Usage Example

```bash
# Start the server
cargo run --release

# In another terminal, connect using telnet or a custom client
telnet localhost <port>

# Example commands:
SET user:1 "Alice"
GET user:1
SETJSON profile:1 '{"name": "Alice", "age": 30}'
JSONGET profile:1 name
```

## Use Cases

- **Event Streaming**: Efficient messaging with streams and queues  
- **High-Speed Caching**: Intelligent caching with multiple eviction policies
- **Session Management**: Storing session information with TTL support

## Architecture Overview

### Core Components

TinyCache's modular architecture consists of:

```
TinyCache (Root Instance)
├── databases (Arc<RwLock<HashMap<String, Arc<RwLock<Cache>>>>>)
│ ├── "default" (Database Name)
│ │ └── Cache (Sharded in-memory storage)
│ │     ├── shards: Vec<Mutex<HashMap<CacheKey, CacheItem>>>
│ │     ├── lru_queues: Vec<Mutex<VecDeque<CacheKey>>>
│ │     ├── document_index: BTreeMap<String, Vec<CacheKey>>
│ │     └── vector_index: VectorIndex (HNSW-like structure)
│ └── ... (Other databases)
├── wal_writer (Write-ahead logging for persistence)
└── auth_manager (Fine-grained access control)
```

### Data Models

TinyCache supports four primary data models within a unified architecture:

1. **Key-Value Store**: Traditional key-value pairs with optional TTL
   - Strings, JSON objects, lists, sets
   - Atomic operations with optimistic concurrency control

2. **Document Store**: JSON-based documents with field indexing
   - Automatic indexing for frequently accessed fields
   - B-tree indexes for O(log n) field lookups

### Workspace and Database Structure

```
TinyCache Instance
├── Workspace: "123"
│   ├── Database: "myapp:kv" (Key-Value)
│   │   └── Cache: { max_size: 10000, eviction_policy: "LRU" }
│   ├── Database: "myapp:docs" (Documents)  
│   │   └── Cache: { max_size: 5000, eviction_policy: "LFRU" }
│   └── Database: "myapp:hybrid" (Multi-Model)
│       └── Cache: { max_size: 15000, supports_all_types: true }
```

## Performance Characteristics

### Sharding and Concurrency

TinyCache employs fine-grained access control through sharding:

- **Shard-Level Locking**: Each shard has its own `RwLock`, enabling parallel access
- **Hash-Based Distribution**: Keys are distributed across shards using bitwise operations
- **Configurable Shard Count**: Typically set to match CPU core count for optimal performance
- **Load Balancing**: Optional load-aware sharding reduces hotspot formation

### Performance Benefits

1. **Increased Concurrency**: Multiple threads operate on different shards simultaneously
2. **Reduced Latency Variability**: Localized contention minimizes wait times
3. **CPU Scaling**: Near-linear scaling up to the number of CPU cores
4. **Memory Efficiency**: Intelligent eviction policies (LRU, LFU, LFRU)

## Key Features

### Caching

- **Multiple Eviction Policies**: LRU, LFU, LFRU (Least Frequently/Recently Used)
- **TTL Support**: Automatic expiration with configurable defaults
- **Frequency Tracking**: Intelligent eviction based on access patterns
- **Memory Management**: Configurable cache sizes per database type

### Indexing System

- **Document Indexing**: Automatic B-tree indexing for JSON fields
- **Query Optimization**: O(log n) field lookups vs O(n) linear scans
- **Dynamic Index Management**: Indexes adapt to usage patterns

### Persistence and Durability

- **Write-Ahead Logging (WAL)**: Ensures data durability
- **Asynchronous Flushing**: Non-blocking persistence operations
- **Crash Recovery**: Automatic restoration from WAL on startup
- **Configurable Persistence**: Adjustable flush intervals

## Supported Commands

### Key-Value Operations
- `SET` - Set a key to hold a string value
- `GET` - Get the value of a key  
- `SETEX` - Set a key with expiration time
- `DELETE` - Remove a key
- `LPUSH` - Insert value at head of list
- `SADD` - Add member to set

### JSON/Document Operations
- `SETJSON` - Set JSON value for key
- `JSONGET` - Get field from JSON value
- `ADD_INDEX` - Add field to indexing system

### Database Management
- `VIEW_DATA` - Display all data in database
- `CHANGE_DB` - Switch current database
- `GET_PERSISTENCE_FILE` - Get current persistence file name

## Installation and Configuration

### System Requirements

- Rust 1.70+ with Tokio async runtime
- Multi-core CPU recommended for optimal sharding performance
- Sufficient RAM for in-memory operations

### Configuration Options

```rust
// Cache Configuration per Database Type
CacheEntryType::KeyValue => (max_size: 10000, ttl: 7_days)
CacheEntryType::Document => (max_size: 5000, indexing: auto)
CacheEntryType::PubSubChannel => (max_size: 2000, max_subs: 1000)
CacheEntryType::EventStream => (max_size: 4000, max_stream_size: 10000)
CacheEntryType::MessageQueue => (max_size: 3000, max_queue_size: 5000)
CacheEntryType::Hybrid => (max_size: 15000, supports_all: true)
```

### Server Architecture

- **TCP-based Server**: Asynchronous I/O using Tokio runtime
- **Concurrent Connections**: Each client handled in separate task
- **Thread Pool**: Configurable worker threads via `mpsc` channels
- **Graceful Shutdown**: Proper resource cleanup and WAL flushing

## Security and Access Control

TinyCache implements fine-grained access control:

- **Multi-database Support**: Isolated workspaces with access tokens
- **Authentication**: Token-based access management
- **Database-Level Permissions**: Granular control over operations
- **Session Management**: Secure session handling with expiration

## Future Enhancements

- **Advanced Vector Indexing**: Full HNSW implementation for O(log n) similarity searches
- **Distributed Mode**: Clustering support for horizontal scaling
- **Query Language**: Simple DSL for complex document queries
- **Compression**: Memory footprint reduction for large datasets
- **Replication**: Master-slave replication for high availability

## Technical Implementation

### Memory Safety and Performance

- **Zero-Cost Abstractions**: Rust's compile-time optimizations
- **Ownership Model**: Eliminates null pointer dereferences and data races
- **Arc and Mutex**: Thread-safe reference counting and locking
- **Async Runtime**: Event-driven model with configurable thread pools

### Complexity Analysis

- **Insert**: O(1) average per shard + O(log n) for document indexing
- **Get**: O(1) average with LRU updates
- **Document Query**: O(log n + k) where k = number of matches
- **Vector Search**: O(n * d) linear (upgradeable to O(log n) with HNSW)
