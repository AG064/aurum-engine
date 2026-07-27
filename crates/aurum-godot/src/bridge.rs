//! Conversion between Godot `Variant` and `serde_json::Value`.
//!
//! We support the obvious types: nil, bool, int, float, String, Dictionary,
//! Array. Everything else becomes `Value::Null`.

use godot::prelude::*;
use serde_json::Value;

/// Convert a `serde_json::Value` into a Godot `Variant`.
pub fn json_to_variant(v: &Value) -> Variant {
    match v {
        Value::Null => Variant::nil(),
        Value::Bool(b) => (*b).to_variant(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_variant()
            } else if let Some(u) = n.as_u64() {
                (u as i64).to_variant()
            } else if let Some(f) = n.as_f64() {
                f.to_variant()
            } else {
                0.to_variant()
            }
        }
        Value::String(s) => s.clone().to_variant(),
        Value::Array(arr) => {
            let mut var_arr = VarArray::new();
            for item in arr {
                let v = json_to_variant(item);
                var_arr.push(&v);
            }
            var_arr.to_variant()
        }
        Value::Object(obj) => {
            let mut dict = Dictionary::<GString, Variant>::new();
            for (k, v) in obj {
                let key_gs = GString::from(k.as_str());
                let val_var = json_to_variant(v);
                dict.set(&key_gs, &val_var);
            }
            dict.to_variant()
        }
    }
}

/// Convert a Godot `Variant` into a `serde_json::Value`.
pub fn variant_to_json(v: &Variant) -> Value {
    // Nil
    if v.is_nil() {
        return Value::Null;
    }
    // Bool
    if let Ok(b) = v.try_to::<bool>() {
        return Value::Bool(b);
    }
    // Integer
    if let Ok(i) = v.try_to::<i64>() {
        return Value::Number(i.into());
    }
    // Float
    if let Ok(f) = v.try_to::<f64>() {
        return serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null);
    }
    // String
    if let Ok(s) = v.try_to::<GString>() {
        return Value::String(s.to_string());
    }
    // Dictionary
    if let Ok(d) = v.try_to::<Dictionary<GString, Variant>>() {
        let mut obj = serde_json::Map::new();
        for key in d.keys_array().iter_shared() {
            let key_str = key.to_string();
            let val = d.get(&key).unwrap_or(Variant::nil());
            obj.insert(key_str, variant_to_json(&val));
        }
        return Value::Object(obj);
    }
    // Array
    if let Ok(arr) = v.try_to::<VarArray>() {
        let mut vec = Vec::new();
        for item in arr.iter_shared() {
            vec.push(variant_to_json(&item));
        }
        return Value::Array(vec);
    }
    // PackedByteArray, etc. — fall through to Null.
    Value::Null
}
