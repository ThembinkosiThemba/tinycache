/// package utils.rs contains utility functions for tinycache
///
use colored::*;
use tokio::io;

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    constants::constants::HOME_FOLDER,
    db::db::{DatabaseType, TinyCache},
};

pub fn compute_now_timestamp() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    now
}

pub fn compute_expiry_using_ttl(ttl: Option<Duration>) -> Option<u64> {
    let expiry = ttl.and_then(|duration| {
        (SystemTime::now() + duration)
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs())
    });

    expiry
}

pub fn parse_connection_string(conn_str: &str) -> Result<(String, String, String, String), String> {
    // Format: tinycache://<username>:<password>@database[:type]
    let connection_string_without_prefix = conn_str
        .strip_prefix("tinycache://")
        .ok_or("Invalid connection string format: missing tinycache:// prefix")?;

    let parts: Vec<&str> = connection_string_without_prefix.split('@').collect();
    if parts.len() != 2 {
        return Err("Invalid connection string format: missing @ separator".into());
    }

    // Parsing credentials
    let credentials_part: Vec<&str> = parts[0].split(':').collect();
    if credentials_part.len() != 2 {
        return Err("Invalid credentials format".into());
    }
    let (username, password) = (
        credentials_part[0].to_string(),
        credentials_part[1].to_string(),
    );

    // Parsing database
    let database_and_type_part = parts[1];
    let db_parts: Vec<&str> = database_and_type_part.split(':').collect();
    if db_parts.len() != 2 {
        return Err("Invalid connection string".into());
    }

    let database = db_parts[0].to_string();
    let db_type = db_parts.get(1).copied().unwrap().to_string();

    Ok((username, password, database, db_type))
}

pub async fn display_startup_info(db: &Arc<TinyCache>) {
    let config = &db.config;
    let mut stats: Vec<(_, _)> = db.get_all_database_stats().await.into_iter().collect();
    stats.sort_by_key(|(_, stat)| stat.entry_count);

    println!("\n{}", "System Configuration".bright_green().bold());
    println!("{}", "====================".bright_green());

    // Server Settings
    println!("\n{}", "Server Configuration:".yellow());
    println!("├─ Host & Port: {}:{}", config.host, config.port);
    println!("├─ Eviction Policy: {}", config.eviction_policy);
    println!("├─ Default TTL: {}", config.default_ttl_secs);
    println!("└─ Max Connections: {}", config.max_connections);

    // Worker Configuration
    println!("\n{}", "Worker Configuration:".yellow());
    println!(
        "├─ Worker Threads: {} & CPUs: {}",
        config.worker_threads,
        num_cpus::get()
    );
    println!(
        "└─ Thread Utilization: {}%",
        (config.worker_threads as f32 / num_cpus::get() as f32 * 100.0) as u32
    );

    // Database and Sharding Stats
    println!("\n{}", "Database and Sharding Stats:".yellow());
    // let databases = db.databases.read().await;
    if stats.is_empty() {
        println!("└─ No databases initialized yet.");
    } else {
        for (i, (db_name, stat)) in stats.iter().enumerate() {
            let prefix = if i == stats.len() - 1 {
                "└─"
            } else {
                "├─"
            };
            println!(
                "{} {}: {} entries",
                prefix,
                db_name.green(),
                stat.entry_count.to_string().yellow()
            );

            if let Some(cache) = db.databases.get(db_name) {
                let cache = cache.read().await;
                let shard_count = cache.shard_count;

                println!("│  ├─ Shard Count: {}", shard_count);
                println!("│  ├─ Eviction Policy: {}", stat.eviction_policy);

                // Total entries consistency check
                let mut total_shard_entries: usize = 0;
                for shard in cache.shards.iter() {
                    total_shard_entries += shard.read().await.len();
                }
                println!(
                    "│  ├─ Total Shard Entries: {} {}",
                    total_shard_entries,
                    if total_shard_entries == stat.entry_count {
                        "(consistent)".green()
                    } else {
                        "(inconsistent)".red()
                    }
                );

                // Shard load distribution
                println!("│  ├─ Shard Load Distribution:");
                for shard_idx in 0..shard_count {
                    let shard_size = cache.shards[shard_idx].read().await.len();
                    let percentage = if stat.entry_count > 0 {
                        (shard_size as f32 / stat.entry_count as f32 * 100.0) as u32
                    } else {
                        0
                    };
                    println!(
                        "│  │  ├─ Shard {}: {} entries ({}%)",
                        shard_idx, shard_size, percentage
                    );
                }

                // LRU Queue stats
                let mut total_lru_entries: usize = 0;
                for queue in cache.lru_queues.iter() {
                    total_lru_entries += queue.read().await.len(); // Async lock each queue
                }
                println!(
                    "│  └─ LRU Queue Entries: {} {}",
                    total_lru_entries,
                    if total_lru_entries == stat.entry_count {
                        "(consistent)".green()
                    } else {
                        "(inconsistent)".red()
                    }
                );
            }
        }
    }

    println!(
        "\n{}",
        "Database is ready to accept connections."
            .bright_green()
            .bold()
    );
    println!("\n");
}

// Get home directory and create .tinycache directory for database configurations and management
pub fn return_data_dir() -> Result<PathBuf, io::Error> {
    let home_dir = dirs::home_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "could not find home directory"))?;
    let data_dir = home_dir.join(HOME_FOLDER);

    Ok(data_dir)
}

pub fn _return_var_dir() -> Result<PathBuf, io::Error> {
    #[cfg(target_os = "linux")]
    let var_dir = PathBuf::from("/var/lib/tinycache");

    #[cfg(target_os = "macos")]
    let var_dir = PathBuf::from("/usr/local/var/tinycache");

    #[cfg(target_os = "windows")]
    let var_dir = {
        let program_data = std::env::var("PROGRAMDATA")
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "could not find %PROGRAMDATA%"))?;
        PathBuf::from(program_data).join("T")
    };

    Ok(var_dir)
}

/// generate_document_id generates a document id for
/// document type databases
///
/// It uses the uuid package to do so
pub fn _generate_document_id() -> String {
    use uuid::Uuid;
    Uuid::new_v4().to_string()
}

/// compute_expiry computes the expiry time using the default
/// ttl which is 7 days.
pub fn compute_expiry() -> Option<Duration> {
    SystemTime::now()
        .checked_add(Duration::from_secs(7 * 24 * 60 * 60))
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| Duration::from_secs(duration.as_secs()))
}

/// set_database_context sets the current database context
/// this ensures requests are processed for the intended
/// database
///
/// The database type is also set as well for proper request routing and handling
///
/// The context uses the username, password and database name
/// in the format username:password@database
pub fn set_database_context(
    connection_string: &str,
) -> Result<(String, DatabaseType), &'static str> {
    // Expected format: tinycache://<username>:<password>@database:type
    // Remove the protocol prefix first
    let without_protocol = connection_string
        .strip_prefix("tinycache://")
        .ok_or("Missing tinycache:// prefix")?;

    // Split at '@' to separate credentials from database info
    let parts: Vec<&str> = without_protocol.split('@').collect();
    if parts.len() != 2 {
        return Err("Invalid connection string format: missing '@' separator");
    }

    // parts[0] must be "<username>:<password>"
    let credentials = parts[0];
    if !credentials.contains(':') {
        return Err("Invalid format: credentials must be username:password");
    }

    let database_part = parts[1];

    // Split database_part by ':' to get database name and type - both required
    let db_parts: Vec<&str> = database_part.split(':').collect();
    if db_parts.len() != 2 {
        return Err("Invalid format: must include database:type");
    }

    let database = db_parts[0];
    if database.is_empty() {
        return Err("Database name cannot be empty");
    }

    let db_type_str = db_parts[1];
    if db_type_str.is_empty() {
        return Err("Database type cannot be empty");
    }

    let db_type = DatabaseType::from_str(db_type_str);

    // Combine credentials and database for the full database name
    Ok((format!("{}@{}", credentials, database), db_type))
}

pub fn print_art() {
    let art = r#"
 ████████╗██╗███╗   ██╗██╗   ██╗ ██████╗ █████╗  ██████╗██╗  ██╗███████╗
 ╚══██╔══╝██║████╗  ██║╚██╗ ██╔╝██╔════╝██╔══██╗██╔════╝██║  ██║██╔════╝
    ██║   ██║██╔██╗ ██║ ╚████╔╝ ██║     ███████║██║     ███████║█████╗  
    ██║   ██║██║╚██╗██║  ╚██╔╝  ██║     ██╔══██║██║     ██╔══██║██╔══╝  
    ██║   ██║██║ ╚████║   ██║   ╚██████╗██║  ██║╚██████╗██║  ██║███████╗
    ╚═╝   ╚═╝╚═╝  ╚═══╝   ╚═╝    ╚═════╝╚═╝  ╚═╝ ╚═════╝╚═╝  ╚═╝╚══════╝
    "#;
    println!("{}", art.bright_blue());
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_connection_string_valid() {
        let conn_str = "tinycache://user:pass@database:kv";
        let result = parse_connection_string(conn_str);

        assert!(result.is_ok());

        let (username, password, database, db_type) = result.unwrap();
        assert_eq!(username, "user");
        assert_eq!(password, "pass");
        assert_eq!(database, "database");
        assert_eq!(db_type, "kv");
    }

    #[test]
    fn test_parse_connection_string_missing_prefix() {
        let conn_str = "user:pass@workspace:ws123/database";
        let result = parse_connection_string(conn_str);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "Invalid connection string format: missing tinycache:// prefix"
        );
    }

    #[test]
    fn test_parse_connection_string_invalid_creds() {
        let conn_str = "tinycache://user@workspace:ws123/database";
        let result = parse_connection_string(conn_str);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Invalid credentials format");
    }

    // Tests for return_data_dir
    #[test]
    fn test_return_data_dir() {
        let result = return_data_dir();
        assert!(result.is_ok());
        let data_dir = result.unwrap();
        let expected = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/home/user"))
            .join(".tinycache");
        assert_eq!(data_dir, expected);
    }

    // Tests for compute_expiry
    #[test]
    fn test_compute_expiry() {
        let expiry = compute_expiry().unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expected = now + (7 * 24 * 60 * 60); // 7 days in seconds
        assert!(expiry.as_secs() >= expected - 1 && expiry.as_secs() <= expected + 1);
    }
}
