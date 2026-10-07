use serde_json::Value;

/// Vapi list endpoints return a bare array or `{ results | data | items }`.
pub fn as_list(value: Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items,
        Value::Object(map) => map
            .get("results")
            .or_else(|| map.get("data"))
            .or_else(|| map.get("items"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

pub fn decode_list<T: serde::de::DeserializeOwned>(value: Value) -> Result<Vec<T>, serde_json::Error> {
    as_list(value)
        .into_iter()
        .map(serde_json::from_value)
        .collect()
}
