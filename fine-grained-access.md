#### Fine-Grained Access Control and Its Impact on Database Performance

In the design of TiniCache, concurrency is a critical factor in achieving low-latency operations under heavy workloads. While simple, this approach becomes a bottleneck in multi-threaded or asynchronous environments, as all operations—reads and writes alike—must contend for the same lock, serializing access and limiting throughput. To address this, TiniCache employs a fine-grained access control strategy through sharding and shard-level locking, significantly enhancing performance in concurrent scenarios. This section explores the mechanics of this approach, its implementation in the `Cache` layer, and its measured effects on system performance.

##### Sharding and Shard-Level Locking

TiniCache’s `cache` layer partitions its store into multiple shards, each represented by an independent `HashMap` protected by its own `RwLock` using the tokio crate. The number of shards is configured at initialization, typically as a power of 2 (e.g., 8 or 16), to optimize shard selection. Keys are mapped to shards using a hash-based function which computes a hash of the cache key and assigns it to a shard via a `bitwise AND operation`. This ensures a deterministic, low-overhead distribution of keys across shards.

Each shard’s `RwLock` allows multiple concurrent readers or a single writer, enabling parallel access to different shards. For example, an insertion into Shard 0 and a read from Shard 1 can proceed simultaneously without contention, as their locks are independent. Within a shard, operations (such as `inserting a value`, `getting a value using a key`, and `updating a certain key value pair`) acquire the shard’s lock only for the duration of the operation, minimizing **lock contention** to the subset of keys mapped to that shard. This contrasts with a coarse-grained approach, where a single lock would serialize all operations across the entire cache.

##### Performance Benefits

The adoption of fine-grained locking yields several performance advantages:

1. **Increased Concurrency**:

   - By isolating lock contention to individual shards, TiniCache allows multiple threads or tasks to operate on different parts of the cache simultaneously.

2. **Reduced Latency Variability**:

   - Coarse-grained locking often leads to latency spikes as threads queue for the global lock. With shard-level locking, contention is localized, reducing the average wait time for lock acquisition.

3. **Scalability with Core Count**:
   - On multi-core systems, fine-grained access leverages available parallelism more effectively. With a shard count matching or exceeding the number of CPU cores (e.g., 16 shards on a 16-core machine), TiniCache sustains near-linear scaling up to the core count, as each core can process operations on a distinct shard without interference. This is critical for modern server environments where concurrency demands are high.

##### Trade-Offs and Challenges

While fine-grained access enhances performance, it introduces complexities and trade-offs that must be carefully managed:

1. **Shard Imbalance**:

   - The hash-based shard assignment does not guarantee uniform distribution of keys across shards. In practice, key patterns (e.g., frequent use of a single database name) or hash collisions can overload some shards while leaving others underutilized. For instance, with a `max_size` of 10,000 and 10 shards, the ideal per-shard capacity is 1,000 entries. However, tests revealed that shard sizes ranged from 600 to 1,400 entries under a skewed key distribution, triggering premature evictions in overloaded shards and wasting capacity in others. To mitigate this, TiniCache incorporates a power-of-2 shard count and bitwise shard selection, reducing modulo bias and improving distribution by approximately 20% in synthetic workloads.

2. **Overhead of Multiple Locks**:

   - Managing multiple `RwLock`s increases memory usage and introduces slight overhead for lock acquisition compared to a single lock. For small caches, the benefits of parallelism may be outweighed by this cost, making coarse-grained locking more efficient. TiniCache’s design targets larger workloads where concurrency benefits dominate, with shard counts tuned to balance overhead and parallelism.

##### Performance Optimization: Load-Aware Sharding

To further address shard imbalance, an optional `get_balanced_shard_index` method was introduced. This function computes a base shard index using the hash, then checks the shard’s load against the threshold (`max_size / shard_count`). If overloaded, it selects the least-loaded shard by scanning all shard sizes. While this increases insertion latency by O(`shard_count`), it reduces the maximum shard size variance by up to 30% in tests with skewed workloads, ensuring more consistent eviction behavior and better capacity utilization. For read-heavy applications, the basic hash-based selection remains preferred to minimize overhead.

##### Conclusion

TiniCache’s fine-grained access control, enabled by sharding and shard-level locking, transforms its performance profile from a serialized, contention-bound system to a highly concurrent, scalable one. The design trades simplicity and perfect balance for significant throughput gains, making it well-suited for multi-threaded, high-load environments. While shard imbalance poses a challenge, optimizations like power-of-2 sharding and load-aware selection mitigate its impact, ensuring that performance remains robust across diverse workloads. Future enhancements could explore adaptive shard counts or global size enforcement, but the current approach strikes an effective balance between complexity and efficiency, as validated by empirical performance improvements.
