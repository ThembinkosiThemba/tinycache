//! # Query Language Module
//!
//! This module defines a query language for aggregating and filtering JSON data stored in `TinyCache`.
//! It supports a variety of aggregation operations and filter conditions, executed via the `QUERY`
//! command in `process_valid_requests`. The language operates on key-value pairs and documents,
//! returning results as JSON.
//!
//! ## Usage
//! Queries are space-separated commands passed to `process_valid_requests` under the `QUERY` prefix.
//! Example: `QUERY FILTER #age gt 30 COUNT SUM #score`
//! - Use `#` prefix for field names (e.g., `#age` for "age" field).
//! - Nested fields use JSON pointer syntax (e.g., `#/user/age`).
//! - Operations are applied sequentially, with filters narrowing the dataset first.
//!
//! ## Aggregation Operations
//! - **COUNT**: Returns the number of items in the current dataset.
//!   - Syntax: `COUNT`
//!   - Example: `QUERY COUNT` -> `{"count": 10}`
//! - **SUM <field>**: Sums numeric values of a field.
//!   - Syntax: `SUM #field`
//!   - Example: `QUERY SUM #score` -> `{"sum_score": 150.0}`
//! - **AVERAGE <field>**: Computes the average of a numeric field.
//!   - Syntax: `AVG #field`
//!   - Example: `QUERY AVG #age` -> `{"avg_age": 25.5}`
//! - **GROUPBY <field>**: Groups data by a field, counting occurrences.
//!   - Syntax: `GROUPBY #field`
//!   - Example: `QUERY GROUPBY #city` -> `{"groups_by_city": [{"value": "NY", "count": 5}, ...]}`
//! - **FILTER <field> <operator> <value>**: Narrows data based on a condition (see Filters below).
//!   - Syntax: `FILTER #field operator value`
//!   - Example: `QUERY FILTER #age gt 30`
//! - **MIN <field>**: Finds the minimum numeric value of a field.
//!   - Syntax: `MIN #field`
//!   - Example: `QUERY MIN #score` -> `{"min_score": 10.0}`
//! - **MAX <field>**: Finds the maximum numeric value of a field.
//!   - Syntax: `MAX #field`
//!   - Example: `QUERY MAX #score` -> `{"max_score": 90.0}`
//! - **DISTINCT <field>**: Lists unique values of a field.
//!   - Syntax: `DISTINCT #field`
//!   - Example: `QUERY DISTINCT #name` -> `{"distinct_name": ["Alice", "Bob"]}`
//! - **TOPN <n> <field>**: Returns the top N values of a field (descending).
//!   - Syntax: `TOPN n #field`
//!   - Example: `QUERY TOPN 3 #score` -> `{"top_3_score": [90, 85, 80]}`
//! - **BOTTOMN <n> <field>**: Returns the bottom N values of a field (ascending).
//!   - Syntax: `BOTTOMN n #field`
//!   - Example: `QUERY BOTTOMN 2 #age` -> `{"bottom_2_age": [18, 19]}`
//! - **MEDIAN <field>**: Computes the median value of a numeric field.
//!   - Syntax: `MEDIAN #field`
//!   - Example: `QUERY MEDIAN #age` -> `{"median_age": 27.0}`
//! - **STDDEV <field>**: Calculates the standard deviation of a numeric field.
//!   - Syntax: `STDDEV #field`
//!   - Example: `QUERY STDDEV #score` -> `{"stddev_score": 12.34}`
//! - **SORT <field> <direction>**: Sorts data by a field (asc or desc).
//!   - Syntax: `SORT #field asc|desc`
//!   - Example: `QUERY SORT #name asc` -> `{"sorted_data": [{"name": "Alice"}, ...]}`
//! - **JOIN <source_key> <source_field> <target_field>**: Joins current data with another dataset.
//!   - Syntax: `JOIN source_key #source_field #target_field`
//!   - Example: `QUERY JOIN users #id #user_id` -> `{"joined_data": [{"id": "1", "name": "Alice"}]}`
//!
//! ## Filter Operators
//! Used with `FILTER` to refine the dataset:
//! - **eq**: Equals (e.g., `FILTER #age eq 25`)
//! - **neq**: Not equals
//! - **gt**: Greater than (numeric)
//! - **lt**: Less than (numeric)
//! - **gte**: Greater than or equal (numeric)
//! - **lte**: Less than or equal (numeric)
//! - **contains**: String contains substring
//! - **startsWith**: String starts with substring
//! - **endsWith**: String ends with substring
//! - **in**: Value is in an array (e.g., `FILTER #id in [1, 2, 3]`)
//! - **notin**: Value is not in an array
//! - **exists**: Field exists
//! - **notexists**: Field does not exist
//! - **regex**: Matches a regex pattern (e.g., `FILTER #name regex ^A.*`)
//! - **between**: Value is within a range (e.g., `FILTER #age between [20, 30]`)
//! - **like**: Matches wildcard pattern (e.g., `FILTER #name like %son`)
//! - **isnull**: Field is null (e.g., `FILTER #email isnull`)
//!
//! ## Implementation Details
//! - Data is sourced from `TinyCache` cache (key-value JSON and documents).
//! - Operations are applied in order, with `filtered_data` updated incrementally.
//! - Results are aggregated into a JSON object with operation-specific keys.
//! - Errors are returned if parameters are missing or invalid.
//!
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

use crate::db::{
    cache::CacheValue,
    db::{DataValue, TinyCache},
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum AggregationOperation {
    Count,
    Sum(String),
    Average(String),
    GroupBy(String),
    Filter(FilterCondition),
    Min(String),
    Max(String),
    Distinct(String),
    TopN {
        field: String,
        n: usize,
    },
    BottomN {
        field: String,
        n: usize,
    },
    Median(String),
    StdDev(String),
    Sort {
        field: String,
        descending: bool,
    },
    Join {
        source_key: String,
        source_field: String,
        target_field: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FilterCondition {
    pub field: String,    // field path in value
    pub operator: String, // "eq", "gt", "lt", "gte", "lte", "contains"
    pub value: JsonValue,
}

fn apply_filter(doc: &JsonValue, condition: &FilterCondition) -> bool {
    if let Some(field_value) = doc.pointer(&condition.field) {
        match condition.operator.as_str() {
            "eq" => field_value == &condition.value,
            "neq" => field_value != &condition.value,
            "gt" => match (field_value.as_f64(), condition.value.as_f64()) {
                (Some(f), Some(c)) => f > c,
                _ => false,
            },
            "lt" => match (field_value.as_f64(), condition.value.as_f64()) {
                (Some(f), Some(c)) => f < c,
                _ => false,
            },
            "gte" => match (field_value.as_f64(), condition.value.as_f64()) {
                (Some(f), Some(c)) => f >= c,
                _ => false,
            },
            "lte" => match (field_value.as_f64(), condition.value.as_f64()) {
                (Some(f), Some(c)) => f <= c,
                _ => false,
            },
            "contains" => field_value
                .as_str()
                .map(|s| s.contains(condition.value.as_str().unwrap_or("")))
                .unwrap_or(false),
            "startsWith" => field_value
                .as_str()
                .map(|s| s.starts_with(condition.value.as_str().unwrap_or("")))
                .unwrap_or(false),
            "endsWith" => field_value
                .as_str()
                .map(|s| s.ends_with(condition.value.as_str().unwrap_or("")))
                .unwrap_or(false),

            "in" => match &condition.value {
                JsonValue::Array(arr) => arr.contains(field_value),
                _ => false,
            },
            "notin" => match &condition.value {
                JsonValue::Array(arr) => !arr.contains(field_value),
                _ => false,
            },
            "exists" => true,
            "notexists" => false,
            "regex" => match (field_value.as_str(), condition.value.as_str()) {
                (Some(f), Some(pattern)) => regex::Regex::new(pattern)
                    .map(|re| re.is_match(f))
                    .unwrap_or(false),
                _ => false,
            },
            "between" => match (&condition.value, field_value.as_f64()) {
                (JsonValue::Array(arr), Some(f)) if arr.len() == 2 => {
                    if let (Some(low), Some(high)) = (arr[0].as_f64(), arr[1].as_f64()) {
                        f >= low && f <= high
                    } else {
                        false
                    }
                }
                _ => false,
            },
            "like" => match (field_value.as_str(), condition.value.as_str()) {
                (Some(f), Some(pattern)) => {
                    let pattern = pattern.replace("%", ".*").replace("_", ".");
                    regex::Regex::new(&format!("^{}$", pattern))
                        .map(|re| re.is_match(f))
                        .unwrap_or(false)
                }
                _ => false,
            },
            "isnull" => field_value.is_null(),

            _ => false,
        }
    } else {
        match condition.operator.as_str() {
            "notexists" => true,
            _ => false,
        }
    }
}

// Helper function to match documents against a filter
// pub fn matches_filter(document: &JsonValue, filter: &JsonValue) -> bool {
//     if let (Some(doc_obj), Some(filter_obj)) = (document.as_object(), filter.as_object()) {
//         filter_obj.iter().all(|(key, filter_value)| {
//             doc_obj
//                 .get(key)
//                 .map_or(false, |doc_value| doc_value == filter_value)
//         })
//     } else {
//         false
//     }
// }

pub async fn aggregate(
    database: &str,
    operations: Vec<AggregationOperation>,
    db: &TinyCache,
) -> JsonValue {
    let cache = db.get_cache(database).await;
    let cache_lock = cache.read().await;
    let mut result = json!({});

    let mut filtered_data: Vec<JsonValue> = Vec::new();
    for shard in cache_lock.shards.iter() {
        let shard_guard = shard.read().await;
        filtered_data.extend(
            shard_guard
                .iter()
                .filter(|(k, _)| k.database == database)
                .filter_map(|(_, item)| match &item.value {
                    CacheValue::KeyValue(entry, _) => {
                        if let DataValue::Json(json) = &entry {
                            Some(json.clone())
                        } else {
                            None
                        }
                    }
                }),
        );
    }

    for operation in operations {
        match operation {
            AggregationOperation::Filter(condition) => {
                let filtered = filtered_data
                    .iter()
                    .filter(|doc| apply_filter(doc, &condition))
                    .cloned()
                    .collect::<Vec<_>>();
                filtered_data = filtered;
            }
            AggregationOperation::Count => {
                result["count"] = json!(filtered_data.len());
            }
            AggregationOperation::Sum(field) => {
                let sum: f64 = filtered_data
                    .iter()
                    .filter_map(|doc| {
                        doc.pointer(&field)
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                    })
                    .sum();
                result[format!("sum_{}", field.replace("/", "_"))] = json!(sum);
            }
            AggregationOperation::Average(field) => {
                let values: Vec<f64> = filtered_data
                    .iter()
                    .filter_map(|doc| {
                        doc.pointer(&field)
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                    })
                    .collect();

                if !values.is_empty() {
                    let avg = values.iter().sum::<f64>() / values.len() as f64;
                    result[format!("avg_{}", field.replace("/", "_"))] = json!(avg);
                }
            }
            AggregationOperation::GroupBy(field) => {
                let mut groups: HashMap<String, Vec<&serde_json::Value>> = HashMap::new();

                for json in &filtered_data {
                    if let Some(group_value) = json.pointer(&field) {
                        let group_key = match group_value {
                            serde_json::Value::String(s) => s.to_string(),
                            serde_json::Value::Number(n) => n.to_string(),
                            serde_json::Value::Bool(b) => b.to_string(),
                            serde_json::Value::Null => "null".to_string(),
                            _ => group_value.to_string(),
                        };
                        groups.entry(group_key).or_insert_with(Vec::new).push(json);
                    }
                }

                let group_stats = groups
                    .into_iter()
                    .map(|(key, entries)| {
                        json!({
                            "value": key,
                            "count": entries.len(),
                        })
                    })
                    .collect::<Vec<_>>();

                result[format!("groups_by_{}", field.replace("/", "_"))] = json!(group_stats);
            }
            AggregationOperation::Min(field) => {
                if let Some(min_value) = filtered_data
                    .iter()
                    .filter_map(|json| {
                        json.pointer(&field)
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                    })
                    .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                {
                    result[format!("min_{}", field.replace("/", "_"))] = json!(min_value);
                }
            }
            AggregationOperation::Max(field) => {
                if let Some(max_value) = filtered_data
                    .iter()
                    .filter_map(|json| {
                        json.pointer(&field)
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                    })
                    .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                {
                    result[format!("max_{}", field.replace("/", "_"))] = json!(max_value);
                }
            }
            AggregationOperation::Distinct(field) => {
                let distinct_values: Vec<JsonValue> = filtered_data
                    .iter()
                    .filter_map(|json| json.pointer(&field).map(|v| v.clone()))
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect();
                result[format!("distinct_{}", field.replace("/", "_"))] = json!(distinct_values);
            }

            AggregationOperation::TopN { field, n } => {
                let mut values: Vec<_> = filtered_data
                    .iter()
                    .filter_map(|json| json.pointer(&field).map(|v| v.clone()))
                    .collect();
                values.sort_by(|a, b| match (a.as_f64(), b.as_f64()) {
                    (Some(a_val), Some(b_val)) => b_val
                        .partial_cmp(&a_val)
                        .unwrap_or(std::cmp::Ordering::Equal),
                    _ => std::cmp::Ordering::Equal,
                });
                result[format!("top_{}_{}", n, field.replace("/", "_"))] =
                    json!(values.into_iter().take(n).collect::<Vec<_>>());
            }

            AggregationOperation::BottomN { field, n } => {
                let mut values: Vec<_> = filtered_data
                    .iter()
                    .filter_map(|json| json.pointer(&field).map(|v| v.clone()))
                    .collect();
                values.sort_by(|a, b| match (a.as_f64(), b.as_f64()) {
                    (Some(a_val), Some(b_val)) => b_val
                        .partial_cmp(&a_val)
                        .unwrap_or(std::cmp::Ordering::Equal),
                    _ => std::cmp::Ordering::Equal,
                });
                result[format!("bottom_{}_{}", n, field.replace("/", "_"))] =
                    json!(values.into_iter().take(n).collect::<Vec<_>>());
            }
            AggregationOperation::Median(field) => {
                let mut values: Vec<f64> = filtered_data
                    .iter()
                    .filter_map(|doc| {
                        doc.pointer(&field)
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                    })
                    .collect();
                if !values.is_empty() {
                    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let mid = values.len() / 2;
                    let median = if values.len() % 2 == 0 {
                        (values[mid - 1] + values[mid]) / 2.0
                    } else {
                        values[mid]
                    };
                    result[format!("median_{}", field.replace("/", "_"))] = json!(median);
                }
            }
            AggregationOperation::StdDev(field) => {
                let values: Vec<f64> = filtered_data
                    .iter()
                    .filter_map(|doc| {
                        doc.pointer(&field)
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                    })
                    .collect();

                if values.len() > 1 {
                    let mean = values.iter().sum::<f64>() / values.len() as f64;
                    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                        / (values.len() - 1) as f64;
                    let std_dev = variance.sqrt();
                    result[format!("stddev_{}", field.replace("/", "_"))] = json!(std_dev);
                }
            }
            AggregationOperation::Sort { field, descending } => {
                filtered_data.sort_by(|a, b| {
                    let a_val = a.pointer(&field).unwrap_or(&JsonValue::Null);
                    let b_val = b.pointer(&field).unwrap_or(&JsonValue::Null);
                    let cmp = match (a_val.as_f64(), b_val.as_f64()) {
                        (Some(a), Some(b)) => {
                            a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
                        }
                        _ => a_val.to_string().cmp(&b_val.to_string()),
                    };
                    if descending {
                        cmp.reverse()
                    } else {
                        cmp
                    }
                });
                result["sorted_data"] = json!(filtered_data.clone());
            }
            AggregationOperation::Join {
                source_key,
                source_field,
                target_field,
            } => {
                let source_data: Vec<JsonValue> = {
                    let mut data = Vec::new();
                    for shard in cache_lock.shards.iter() {
                        let shard_guard = shard.read().await;
                        data.extend(
                            shard_guard
                                .iter()
                                .filter(|(k, _)| k.database == database && k.key == source_key)
                                .filter_map(|(_, item)| match &item.value {
                                    CacheValue::KeyValue(entry, _) => {
                                        if let DataValue::Json(json) = entry {
                                            Some(json.clone())
                                        } else {
                                            None
                                        }
                                    }
                                }),
                        );
                    }
                    data
                };

                let joined: Vec<JsonValue> = filtered_data
                    .iter()
                    .filter_map(|target| {
                        let target_val = target.pointer(&target_field)?;
                        source_data.iter().find_map(|source| {
                            if source.pointer(&source_field) == Some(target_val) {
                                let mut merged = target.clone();
                                if let (Some(m), Some(s)) =
                                    (merged.as_object_mut(), source.as_object())
                                {
                                    m.extend(s.clone());
                                }
                                Some(merged)
                            } else {
                                None
                            }
                        })
                    })
                    .collect();
                filtered_data = joined;
                result["joined_data"] = json!(filtered_data.clone());
            }
        }
    }

    result
}
