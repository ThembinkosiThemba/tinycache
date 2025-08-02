use serde_json::Value as JsonValue;
use std::time::Duration;

use crate::{
    db::db::{DataValue, DatabaseType, TinyCache},
    query::{
        middleware::query_security_middleware,
        query::{aggregate, AggregationOperation, FilterCondition},
    },
    utils::{
        logs::LogLevel,
        response::{Response, ResponseData},
        utils::set_database_context,
    },
};

pub async fn process_requests(request: String, db: &TinyCache) -> String {
    let parts: Vec<&str> = request.trim().split_whitespace().collect();
    match parts.as_slice() {
        [connection_string, command @ ..] => match set_database_context(connection_string) {
            Ok((database, db_type)) => {
                db.set_current_database(Some(&database)).await;
                let command_str = command.join(" ");

                // Check shared commands first
                if let Some(response) =
                    process_shared_requests(&database, command_str.clone(), db).await
                {
                    response
                } else {
                    match db_type {
                        DatabaseType::KeyValue => {
                            process_key_value_requests(&database, command_str, db).await
                        }
                    }
                }
            }
            Err(e) => format!("Error: {}\r\n", e),
        },
        _ => "Invalid command\r\n".to_string(),
    }
}

async fn process_shared_requests(database: &str, request: String, db: &TinyCache) -> Option<String> {
    let parts: Vec<&str> = request.trim().split_whitespace().collect();
    match parts.as_slice() {
        ["PING"] => Some(Response::success(ResponseData::String("PONG".to_string())).to_string()),

        ////////////////////////////////////////////////////////////////////////////////////////////
        /////////////////////////////////// DB MONITORING && LOGS //////////////////////////////////
        ////////////////////////////////////////////////////////////////////////////////////////////
        ["DBSTATS"] => Some({
            if let Some(stats) = db.get_database_stats(database).await {
                Response::success(ResponseData::Json(serde_json::to_value(stats).unwrap()))
                    .to_string()
            } else {
                Response::error("DATABASE_NOT_FOUND").to_string()
            }
        }),
        ["VIEW_LOGS"] => Some(match db.logger.view_application_logs(&db, database).await {
            Ok(logs) => Response::success(ResponseData::Json(serde_json::to_value(logs).unwrap()))
                .to_string(),
            Err(e) => Response::error(format!("failed to retrieve logs: {}", e)).to_string(),
        }),
        ["VIEW_SYSTEM_LOGS"] => Some(
            match db
                .logger
                .get_logs(&db, None, Some(LogLevel::System), None, None)
                .await
            {
                Ok(logs) => {
                    Response::success(ResponseData::Json(serde_json::to_value(logs).unwrap()))
                        .to_string()
                }
                Err(e) => {
                    Response::error(format!("Failed to retrieve system logs: {}", e)).to_string()
                }
            },
        ),
        ["ALL_DBSTATS"] => Some({
            let stats = db.get_all_database_stats().await;
            Response::success(ResponseData::Json(serde_json::to_value(stats).unwrap())).to_string()
        }),
        ["CLEAR_DB"] => Some(match db.drop_db(database).await {
            Ok(()) => Response::success(ResponseData::String("OK".to_string())).to_string(),
            Err(e) => Response::error(e.to_string()).to_string(),
        }),

        ////////////////////////////////////////////////////////////////////////////////////////////
        ////////////////////////////////////////// QUERY ///////////////////////////////////////////
        ////////////////////////////////////////////////////////////////////////////////////////////

        // The QUERY command processes a sequence of aggregation operations (e.g., COUNT, SUM, FILTER)
        // on JSON data stored in the database. It splits the input into words, iterates over them with
        // an index `i`, and matches each operation. Operations like SUM or FILTER require additional
        // parameters (e.g., field names), checked with `i + n < rest.len()` to ensure validity.
        // Each operation consumes a specific number of words, incrementing `i` accordingly (e.g.,
        // `i += 2` for SUM, `i += 4` for FILTER), then delegates to the `aggregate` function to
        // compute and return the result as JSON.
        ["QUERY", rest @ ..] => Some(
            match query_security_middleware(database, &request, db).await {
                Ok(()) => {
                    if rest.is_empty() {
                        Response::error("MISSING_OPERATIONS").to_string()
                    } else {
                        let mut operations = Vec::new();
                        let mut i = 0;
                        while i < rest.len() {
                            match rest[i] {
                                "COUNT" => {
                                    operations.push(AggregationOperation::Count);
                                    i += 1;
                                }
                                "SUM" => {
                                    if i + 1 < rest.len() {
                                        operations.push(AggregationOperation::Sum(
                                            rest[i + 1].to_string(),
                                        ));
                                        i += 2;
                                    } else {
                                        return Some(
                                            Response::error("MISSING_FIELD_FOR_SUM").to_string(),
                                        );
                                    }
                                }
                                "AVG" => {
                                    if i + 1 < rest.len() {
                                        operations.push(AggregationOperation::Average(
                                            rest[i + 1].to_string(),
                                        ));
                                        i += 2;
                                    } else {
                                        return Some(
                                            Response::error("MISSING_FIELD_FOR_AVERAGE")
                                                .to_string(),
                                        );
                                    }
                                }
                                "GROUPBY" => {
                                    if i + 1 < rest.len() {
                                        operations.push(AggregationOperation::GroupBy(
                                            rest[i + 1].to_string(),
                                        ));
                                        i += 2;
                                    } else {
                                        return Some(
                                            Response::error("MISSING_FIELD_FOR_GROUPBY")
                                                .to_string(),
                                        );
                                    }
                                }
                                "FILTER" => {
                                    if i + 3 < rest.len() {
                                        operations.push(AggregationOperation::Filter(
                                            FilterCondition {
                                                field: rest[i + 1].to_string(),
                                                operator: rest[i + 2].to_string(),
                                                value: match rest[i + 3].parse::<f64>() {
                                                    Ok(n) => JsonValue::Number(
                                                        serde_json::Number::from_f64(n).unwrap(),
                                                    ),
                                                    Err(_) => {
                                                        JsonValue::String(rest[i + 3].to_string())
                                                    }
                                                },
                                            },
                                        ));
                                        i += 4;
                                    } else {
                                        return Some(
                                            Response::error("INVALID_FILTER_FORMAT").to_string(),
                                        );
                                    }
                                }
                                "MIN" => {
                                    if i + 1 < rest.len() {
                                        operations.push(AggregationOperation::Min(
                                            rest[i + 1].to_string(),
                                        ));
                                        i += 2;
                                    } else {
                                        return Some(
                                            Response::error("MISSING_FIELD_FOR_MIN").to_string(),
                                        );
                                    }
                                }
                                "MAX" => {
                                    if i + 1 < rest.len() {
                                        operations.push(AggregationOperation::Max(
                                            rest[i + 1].to_string(),
                                        ));
                                        i += 2;
                                    } else {
                                        return Some(
                                            Response::error("MISSING_FIELD_FOR_MAX").to_string(),
                                        );
                                    }
                                }

                                "DISCTICT" => {
                                    if i + 1 < rest.len() {
                                        operations.push(AggregationOperation::Distinct(
                                            rest[i + 1].to_string(),
                                        ));
                                        i += 2;
                                    } else {
                                        return Some(
                                            Response::error("MISSING_FIELD_FOR_DISTINCT")
                                                .to_string(),
                                        );
                                    }
                                }

                                "TOPN" => {
                                    if i + 2 < rest.len() {
                                        if let Ok(n) = rest[i + 1].parse::<usize>() {
                                            operations.push(AggregationOperation::TopN {
                                                field: rest[i + 2].to_string(),
                                                n,
                                            });
                                            i += 3;
                                        } else {
                                            return Some(
                                                Response::error("INVALID_N_VALUE_FOR_TOPN")
                                                    .to_string(),
                                            );
                                        }
                                    } else {
                                        return Some(
                                            Response::error("MISSING_PARAMETERS_FOR_TOPN")
                                                .to_string(),
                                        );
                                    }
                                }

                                "BOTTOMN" => {
                                    if i + 2 < rest.len() {
                                        if let Ok(n) = rest[i + 1].parse::<usize>() {
                                            operations.push(AggregationOperation::BottomN {
                                                field: rest[i + 2].to_string(),
                                                n,
                                            });
                                            i += 3;
                                        } else {
                                            return Some(
                                                Response::error("INVALID_N_VALUE_FOR_BOTTOMN")
                                                    .to_string(),
                                            );
                                        }
                                    } else {
                                        return Some(
                                            Response::error("MISSING_RAPAMETERS_FOR_BOTTOM")
                                                .to_string(),
                                        );
                                    }
                                }

                                "MEDIAN" => {
                                    if i + 1 < rest.len() {
                                        operations.push(AggregationOperation::Median(
                                            rest[i + 1].to_string(),
                                        ));
                                        i += 2;
                                    } else {
                                        return Some(
                                            Response::error("MISSING_FIELD_FOR_MEDIAN").to_string(),
                                        );
                                    }
                                }

                                "STDDEV" => {
                                    if i + 1 < rest.len() {
                                        operations.push(AggregationOperation::StdDev(
                                            rest[i + 1].to_string(),
                                        ));
                                        i += 2;
                                    } else {
                                        return Some(
                                            Response::error("MISSING_FIELD_FOR_STDDEV").to_string(),
                                        );
                                    }
                                }

                                "SORT" => {
                                    if i + 2 < rest.len() {
                                        let field = rest[i + 1].trim_start_matches('#');
                                        let direction = rest[i + 2].to_lowercase();
                                        if direction == "asc" || direction == "desc" {
                                            operations.push(AggregationOperation::Sort {
                                                field: field.to_string(),
                                                descending: direction == "desc",
                                            });
                                            i += 3;
                                        } else {
                                            return Some(
                                                Response::error(
                                                    "INVALID_SORT_DIRECTION_USE_ASC_OR_DESC",
                                                )
                                                .to_string(),
                                            );
                                        }
                                    } else {
                                        return Some(
                                            Response::error("MISSING_PARAMETERS_FOR_SORT")
                                                .to_string(),
                                        );
                                    }
                                }

                                "JOIN" => {
                                    if i + 3 < rest.len() {
                                        let source_key = rest[i + 1];
                                        let source_field = rest[i + 2];
                                        let target_field = rest[i + 3];
                                        operations.push(AggregationOperation::Join {
                                            source_key: source_key.to_string(),
                                            source_field: source_field.to_string(),
                                            target_field: target_field.to_string(),
                                        });
                                        i += 4;
                                    } else {
                                        return Some(
                                            Response::error("MISSING_PARAMETERS_FOR_JOIN")
                                                .to_string(),
                                        );
                                    }
                                }

                                _ => {
                                    return Some(Response::error("UNKNOWN_OPERATION").to_string());
                                }
                            }
                        }

                        let result = aggregate(database, operations, db).await;
                        Response::success(ResponseData::Json(result)).to_string()
                    }
                }
                Err(e) => Response::error(e).to_string(),
            },
        ),

        _ => None, // Not a shared command, delegate to type-specific handler
    }
}

async fn process_key_value_requests(database: &str, request: String, db: &TinyCache) -> String {
    let parts: Vec<&str> = request.trim().split_whitespace().collect();
    let response = match parts.as_slice() {
        ////////////////////////////////////////////////////////////////////////////////////////////
        /////////////////////////////////////// KEY_VALUE //////////////////////////////////////////
        ////////////////////////////////////////////////////////////////////////////////////////////
        ["SET", key, rest @ ..] => {
            let json_str = rest.join(" ");
            match serde_json::from_str::<JsonValue>(&json_str) {
                Ok(json_value) => match db
                    .create_key_value(database, key.to_string(), DataValue::Json(json_value))
                    .await
                {
                    Ok(_) => Response::success(ResponseData::String("OK".to_string())),
                    Err(e) => Response::error(e.to_string()),
                },
                Err(e) => Response::error(format!("INVALID_JSON: {}", e)),
            }
        }

        ["SET_EX", key, ttl_str, rest @ ..] => {
            if rest.len() >= 1 {
                match ttl_str.parse::<u64>() {
                    Ok(ttl) => {
                        let value_str = rest.join(" ");
                        match serde_json::from_str::<JsonValue>(&value_str) {
                            Ok(json_value) => {
                                match db
                                    .create_key_value_with_ttl(
                                        database,
                                        key.to_string(),
                                        DataValue::Json(json_value),
                                        Duration::from_secs(ttl),
                                    )
                                    .await
                                {
                                    Ok(()) => {
                                        Response::success(ResponseData::String("OK".to_string()))
                                    }
                                    Err(e) => Response::error(e.to_string()),
                                }
                            }

                            Err(_) => {
                                // Try as a simple string if JSON parsing fails
                                match db
                                    .create_key_value_with_ttl(
                                        database,
                                        key.to_string(),
                                        DataValue::String(value_str),
                                        Duration::from_secs(ttl),
                                    )
                                    .await
                                {
                                    Ok(()) => {
                                        Response::success(ResponseData::String("OK".to_string()))
                                    }
                                    Err(e) => Response::error(e.to_string()),
                                }
                            }
                        }
                    }
                    Err(_) => Response::error("INVALID_TTL"),
                }
            } else {
                Response::error("INVALID_COMMAND")
            }
        }
        ["GET_KEY", key] => match db.get_key_value(database, key).await {
            Some(DataValue::String(s)) => Response::success(ResponseData::String(s)),
            Some(DataValue::List(l)) => Response::success(ResponseData::List(l)),
            Some(DataValue::Set(s)) => {
                Response::success(ResponseData::Set(s.keys().cloned().collect()))
            }
            Some(DataValue::Json(j)) => Response::success(ResponseData::Json(j)),
            None => Response::error("NOT_FOUND"),
        },

        ["UPDATE_KEY", key, rest @ ..] => {
            let json_str = rest.join(" ");
            match serde_json::from_str::<JsonValue>(&json_str) {
                Ok(json_value) => match db
                    .update_key_value(database, key, DataValue::Json(json_value), None)
                    .await
                {
                    Ok(Some(_)) => Response::success(ResponseData::String("UPDATED".to_string())),
                    Ok(None) => Response::error("NOT_FOUND"),
                    Err(e) => Response::error(e.to_string()),
                },
                Err(e) => Response::error(format!("INVALID_JSON: {}", e)),
            }
        }

        ["DELETE_KEY", key] => match db.delete_key_value(database, key).await {
            Ok(true) => Response::success(ResponseData::String("DELETED".to_string())),
            Ok(false) => Response::error("NOT_FOUND"),
            Err(e) => Response::error(e.to_string()),
        },

        ["Get_All_KV"] => {
            let data = db.view_data(database).await;
            Response::success(ResponseData::Json(data))
        }

        ["INCR_KEY", key, amount] => match amount.parse::<f64>() {
            Ok(num) => match db.increment_key_value(database, key, num).await {
                Ok(Some(new_value)) => Response::success(ResponseData::Json(JsonValue::Number(
                    serde_json::Number::from_f64(new_value).unwrap(),
                ))),
                Ok(None) => Response::error("NOT_FOUND_OR_NOT_NUMERIC"),
                Err(e) => Response::error(e.to_string()),
            },
            Err(_) => Response::error("INVALID_AMOUNT"),
        },

        ["DECR_KEY", key, amount] => match amount.parse::<f64>() {
            Ok(num) => match db.decrement_key_value(database, key, num).await {
                Ok(Some(new_value)) => Response::success(ResponseData::Json(JsonValue::Number(
                    serde_json::Number::from_f64(new_value).unwrap(),
                ))),
                Ok(None) => Response::error("NOT_FOUND_OR_NOT_NUMERIC"),
                Err(e) => Response::error(e.to_string()),
            },
            Err(_) => Response::error("INVALID_AMOUNT"),
        },

        ["STORE", rest @ ..] => {
            if rest.len() >= 2 {
                let key = rest[0];
                let json_str = rest[1..].join(" ");
                match serde_json::from_str::<JsonValue>(&json_str) {
                    Ok(json_value) => {
                        match db
                            .create_key_value(
                                database,
                                key.to_string(),
                                DataValue::Json(json_value),
                            )
                            .await
                        {
                            Ok(_) => Response::success(ResponseData::String("OK".to_string())),
                            Err(e) => Response::error(e.to_string()),
                        }
                    }
                    Err(e) => Response::error(format!("INVALID_JSON: {}", e)),
                }
            } else {
                Response::error("INVALID_COMMAND")
            }
        }
        _ => Response::error("INVALID_COMMAND"),
    };

    response.to_string()
}

// #[cfg(test)]
// mod tests {
//     use std::path::PathBuf;

//     use crate::security::config::DBConfig;

//     use super::*;

//     // async fn setup() -> TinyCache {
//     //     let config = DBConfig {
//     //         ..Default::default()
//     //     };
//     //     // let db = TinyCache::new(PathBuf::from("./test_db"), config.clone())
//     //         // .await
//     //         // .unwrap();

//     //     // db
//     // }

//     #[tokio::test]
//     async fn test_key_value_commands() {
//         let db = setup().await;

//         // SET
//         let resp =
//             process_requests("default SET key1 {\"value\": \"test\"}".to_string(), &db).await;
//         assert!(resp.contains("OK"));

//         // GET_KEY
//         let resp = process_requests("default GET_KEY key1".to_string(), &db).await;
//         assert!(resp.contains("{\"value\": \"test\"}"));

//         // INCR_KEY
//         _ = process_requests("default SET num 5".to_string(), &db).await;
//         let resp = process_requests("default INCR_KEY num 2".to_string(), &db).await;
//         assert!(resp.contains("7"));
//     }

//     #[tokio::test]
//     async fn test_invalid_command() {
//         let db = setup().await;
//         let resp = process_requests("default UNKNOWN".to_string(), &db).await;
//         assert!(resp.contains("INVALID_COMMAND"));
//     }
// }
