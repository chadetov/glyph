use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct User {
    pub name: String,
    pub age: u32,
}

pub fn parse_user(input: &Value) -> Result<User, String> {
    let obj = input.as_object().ok_or_else(|| "expected object".to_string())?;
    let name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "name: expected string".to_string())?
        .to_string();
    let age = obj
        .get("age")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "age: expected number".to_string())? as u32;
    Ok(User { name, age })
}
