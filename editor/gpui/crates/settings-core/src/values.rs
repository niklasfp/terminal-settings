//! Value checks every terminal's writer shares, so the same mistake reads the same in both apps.

use crate::Setting;
use serde_json::Value;

/// A number as people write it: whole numbers without a decimal point, others in the shortest
/// form that reads back the same ("14", "0.5").
pub fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{n}") }
}

/// The setting's `min`/`max` (none for a field without a setting), as a user-facing error.
pub fn check_range(s: Option<&Setting>, n: f64) -> Result<(), String> {
    if let Some(min) = s.and_then(|s| s.min)
        && n < min
    {
        return Err(format!("Must be at least {}.", format_number(min)));
    }
    if let Some(max) = s.and_then(|s| s.max)
        && n > max
    {
        return Err(format!("Must be at most {}.", format_number(max)));
    }
    Ok(())
}

pub fn boolean(v: &Value) -> Result<bool, String> {
    v.as_bool().ok_or_else(|| "Expected true or false.".to_string())
}

/// A whole number within the setting's range.
pub fn int(s: Option<&Setting>, v: &Value) -> Result<i64, String> {
    let n = number(v)?;
    if n != n.floor() {
        return Err("Must be a whole number.".into());
    }
    check_range(s, n)?;
    Ok(n as i64)
}

/// A number within the setting's range.
pub fn float(s: Option<&Setting>, v: &Value) -> Result<f64, String> {
    let n = number(v)?;
    check_range(s, n)?;
    Ok(n)
}

/// A list without nulls.
pub fn items(v: &Value) -> Result<&Vec<Value>, String> {
    let Value::Array(items) = v else {
        return Err("Expected a list.".into());
    };
    if items.iter().any(Value::is_null) {
        return Err("Lists can't contain null.".into());
    }
    Ok(items)
}

/// A finite number, also from text ("14", " 0.5 ").
pub fn number(v: &Value) -> Result<f64, String> {
    let parsed = match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    parsed.filter(|d| d.is_finite()).ok_or_else(|| "Expected a number.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SettingType;
    use serde_json::json;

    #[test]
    fn numbers_read_and_print_like_people_write_them() {
        assert_eq!(format_number(14.0), "14");
        assert_eq!(format_number(0.5), "0.5");
        assert_eq!(number(&json!(" 0.5 ")), Ok(0.5));
        assert_eq!(number(&json!("x")), Err("Expected a number.".into()));
        assert_eq!(items(&json!([1, null])), Err("Lists can't contain null.".into()));
    }

    #[test]
    fn range_errors_name_the_bound() {
        let mut s = crate::Setting::new("size", "fonts".to_string(), SettingType::Float);
        (s.min, s.max) = (Some(6.0), Some(48.5));
        assert_eq!(float(Some(&s), &json!(5)), Err("Must be at least 6.".into()));
        assert_eq!(float(Some(&s), &json!(49)), Err("Must be at most 48.5.".into()));
        assert_eq!(float(Some(&s), &json!("10")), Ok(10.0));
        assert_eq!(float(None, &json!(1000)), Ok(1000.0));
        assert_eq!(int(Some(&s), &json!(7.5)), Err("Must be a whole number.".into()));
        assert_eq!(boolean(&json!("yes")), Err("Expected true or false.".into()));
    }
}
