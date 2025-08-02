use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "type", content = "data")]
pub enum ResponseData {
    None(String),
    String(String),
    Json(JsonValue),
    List(Vec<String>),
    Set(Vec<String>),
    Session(JsonValue),
    Batch(HashMap<String, String>),
    Error(String),
}

#[derive(Serialize, Deserialize)]
pub struct Response {
    pub status: String,          // "success" or "error"
    pub message: Option<String>, // Optional message for errors or additional info
    pub data: Option<ResponseData>,
}

impl Response {
    pub fn success(data: ResponseData) -> Self {
        Response {
            status: "success".to_string(),
            message: None,
            data: Some(data),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Response {
            status: "error".to_string(),
            message: Some(message.into()),
            data: None,
        }
    }

    pub fn to_string(&self) -> String {
        serde_json::to_string(self).unwrap() + "\r\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    // Helper function to create a sample HashMap for Batch variant
    fn sample_batch_data() -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert("key1".to_string(), "value1".to_string());
        map.insert("key2".to_string(), "value2".to_string());
        map
    }

    // Test ResponseData serialization
    #[test]
    fn test_response_data_serialization() {
        let variants = vec![
            ResponseData::String("test".to_string()),
            ResponseData::Json(json!({"key": "value"})),
            ResponseData::List(vec!["a".to_string(), "b".to_string()]),
            ResponseData::Set(vec!["x".to_string(), "y".to_string()]),
            ResponseData::Session(json!({"session_id": "123"})),
            ResponseData::Batch(sample_batch_data()),
            ResponseData::Error("something went wrong".to_string()),
        ];

        for variant in variants {
            let serialized = serde_json::to_string(&variant).unwrap();
            let deserialized: ResponseData = serde_json::from_str(&serialized).unwrap();
            match (variant, deserialized) {
                (ResponseData::String(s1), ResponseData::String(s2)) => assert_eq!(s1, s2),
                (ResponseData::Json(j1), ResponseData::Json(j2)) => assert_eq!(j1, j2),
                (ResponseData::List(l1), ResponseData::List(l2)) => assert_eq!(l1, l2),
                (ResponseData::Set(s1), ResponseData::Set(s2)) => assert_eq!(s1, s2),
                (ResponseData::Session(s1), ResponseData::Session(s2)) => assert_eq!(s1, s2),
                (ResponseData::Batch(b1), ResponseData::Batch(b2)) => assert_eq!(b1, b2),
                (ResponseData::Error(e1), ResponseData::Error(e2)) => assert_eq!(e1, e2),
                _ => panic!("Deserialized variant doesn't match original"),
            }
        }
    }

    // Test Response::success
    #[test]
    fn test_response_success() {
        let data = ResponseData::String("success data".to_string());
        let response = Response::success(data.clone());
        assert_eq!(response.status, "success");
        assert_eq!(response.message, None);
        assert!(response.data.is_some());
        if let Some(ResponseData::String(s)) = response.data {
            assert_eq!(s, "success data");
        } else {
            panic!("Unexpected data variant");
        }
    }

    // Test Response::error
    #[test]
    fn test_response_error() {
        let response = Response::error("something failed");
        assert_eq!(response.status, "error");
        assert_eq!(response.message, Some("something failed".to_string()));
        assert_eq!(response.data, None);
    }

    // Test Response::to_string with success
    #[test]
    fn test_response_to_string_success() {
        let data = ResponseData::Json(json!({"key": "value"}));
        let response = Response::success(data);
        let result = response.to_string();
        let expected =
            r#"{"status":"success","message":null,"data":{"type":"Json","data":{"key":"value"}}}"#
                .to_owned()
                + "\r\n";
        assert_eq!(result, expected);

        // Verify it can be deserialized back
        let deserialized: Response =
            serde_json::from_str(&result.trim_end_matches("\r\n")).unwrap();
        assert_eq!(deserialized.status, "success");
        assert_eq!(deserialized.message, None);
        if let Some(ResponseData::Json(json)) = deserialized.data {
            assert_eq!(json, json!({"key": "value"}));
        } else {
            panic!("Unexpected data variant");
        }
    }

    // Test Response::to_string with error
    #[test]
    fn test_response_to_string_error() {
        let response = Response::error("test error");
        let result = response.to_string();
        let expected =
            r#"{"status":"error","message":"test error","data":null}"#.to_owned() + "\r\n";
        assert_eq!(result, expected);

        // Verify it can be deserialized back
        let deserialized: Response =
            serde_json::from_str(&result.trim_end_matches("\r\n")).unwrap();
        assert_eq!(deserialized.status, "error");
        assert_eq!(deserialized.message, Some("test error".to_string()));
        assert_eq!(deserialized.data, None);
    }

    // Test Response with all ResponseData variants
    #[test]
    fn test_response_with_all_variants() {
        let variants = vec![
            ResponseData::String("hello".to_string()),
            ResponseData::Json(json!({"id": 1})),
            ResponseData::List(vec!["item1".to_string(), "item2".to_string()]),
            ResponseData::Set(vec!["unique1".to_string(), "unique2".to_string()]),
            ResponseData::Session(json!({"token": "abc123"})),
            ResponseData::Batch(sample_batch_data()),
            ResponseData::Error("internal error".to_string()),
        ];

        for data in variants {
            let response = Response::success(data);
            let serialized = response.to_string();
            assert!(serialized.ends_with("\r\n"));
            let deserialized: Response =
                serde_json::from_str(&serialized.trim_end_matches("\r\n")).unwrap();
            assert_eq!(deserialized.status, "success");
            assert_eq!(deserialized.message, None);
            assert!(deserialized.data.is_some());
        }
    }

    // Test Response::error with different message types
    #[test]
    fn test_response_error_message_types() {
        let error1 = Response::error("string literal");
        assert_eq!(error1.message, Some("string literal".to_string()));

        let error2 = Response::error(String::from("owned string"));
        assert_eq!(error2.message, Some("owned string".to_string()));

        let error3 = Response::error(&"ref string".to_string());
        assert_eq!(error3.message, Some("ref string".to_string()));
    }
}
