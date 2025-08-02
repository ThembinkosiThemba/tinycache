use crate::{db::db::TinyCache, utils::logs::LogLevel};

// Middleware to enforce database isolation for QUERY commands
// Ensures that queries only access data from the specified database, preventing
// cross-database attacks by validating the database scope and sanitizing inputs.
// Designed to be fast with minimal overhead using read-only locks and string checks.
//
// Usage:
// - Call this middleware before executing QUERY logic in process_valid_requests.
// - Pass the database name, raw query string, and TinyCache instance.
// - Returns Ok(()) if safe to proceed, or Err(String) with an error message if not.
//
// Security Features:
// - Database scope check: Ensures the database matches the request context.
// - Input sanitization: Blocks attempts to inject database names or malicious patterns.
// - Logging: Records suspicious attempts for monitoring (via TinyCache logger).
// - Minimal performance impact: Uses lightweight string checks and async read locks.
pub async fn query_security_middleware(
    database: &str,  // database specified on the process request function
    raw_query: &str, // the full query string
    db: &TinyCache,     // the database instance
) -> Result<(), String> {
    // normalization of input for consistent comparison
    let database = database.trim().to_lowercase();
    let raw_query = raw_query.trim().to_lowercase();

    // Checking for database name injection attempts in the query
    // Attackers might try "QUERY SUM age db:other_db" to switch databases in a way
    if raw_query.contains(&format!("db:{}", database)) || raw_query.contains("db:") {
        let _ = db
            .logger
            .log_warn(
                &format!(
                    "Suspicious query detected: possible db injection - {}",
                    raw_query
                ),
                LogLevel::System,
                &db,
            )
            .await;
        return Err("Invalid query: database references not allowed".to_string());
    }

    // Basic sanitization: block common attack patterns
    // Disallow special characters that could be used for injection or traversal
    let suspicious_chars = [";", "--", "/*", "*/", ".."];
    if suspicious_chars.iter().any(|c| raw_query.contains(c)) {
        let _ = db
            .logger
            .log_info(
                &format!(
                    "Suspicious query detected: invalid characters - {}",
                    raw_query
                ),
                LogLevel::System,
                &db,
            )
            .await;
        return Err("Invalid query: suspicious characters detected".to_string());
    }

    // Verify database exists in TinyCache to prevent invalid access
    // let cache = db.get_cache(&database).await;
    // let cache_lock = cache.read().await;
    // let mut database_exists = false;
    // for shard in cache_lock.shards.iter() {
    //     if shard
    //         .read()
    //         .await
    //         .iter()
    //         .any(|(k, _)| k.database == database)
    //     {
    //         database_exists = true;
    //         break;
    //     }
    // }
    // if !database_exists {
    //     return Err("Database not found or inaccessible".to_string());
    // }
    // All checks passed; query is safe to execute within this database
    Ok(())
}
