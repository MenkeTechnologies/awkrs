use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Value {
    Str(String),
    Num(f64),
    Array(HashMap<String, Value>),
}

impl Value {
    pub fn as_str(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    format!("{}", *n as i64)
                } else {
                    format!("{n}")
                }
            }
            Value::Array(_) => "".into(),
        }
    }

    pub fn as_number(&self) -> f64 {
        match self {
            Value::Num(n) => *n,
            Value::Str(s) => s.parse().unwrap_or(0.0),
            Value::Array(_) => 0.0,
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty() && s.parse::<f64>().map(|n| n != 0.0).unwrap_or(true),
            Value::Array(a) => !a.is_empty(),
        }
    }
}

pub struct Runtime {
    pub vars: HashMap<String, Value>,
    pub fields: Vec<String>,
    pub record: String,
    pub nr: f64,
    pub fnr: f64,
    pub filename: String,
}

impl Runtime {
    pub fn new() -> Self {
        let mut vars = HashMap::new();
        vars.insert("OFS".into(), Value::Str(" ".into()));
        vars.insert("ORS".into(), Value::Str("\n".into()));
        vars.insert("OFMT".into(), Value::Str("%.6g".into()));
        Self {
            vars,
            fields: Vec::new(),
            record: String::new(),
            nr: 0.0,
            fnr: 0.0,
            filename: String::new(),
        }
    }

    pub fn set_field_sep_split(&mut self, fs: &str, line: &str) {
        self.record = line.to_string();
        if fs.is_empty() {
            self.fields = line.chars().map(|c| c.to_string()).collect();
        } else if fs == " " {
            self.fields = line.split_whitespace().map(String::from).collect();
        } else {
            self.fields = line.split(fs).map(String::from).collect();
        }
        let nf = self.fields.len() as f64;
        self.vars.insert("NF".into(), Value::Num(nf));
    }

    pub fn field(&self, i: i32) -> Value {
        if i < 0 {
            return Value::Str(String::new());
        }
        let idx = i as usize;
        if idx == 0 {
            return Value::Str(self.record.clone());
        }
        self.fields
            .get(idx - 1)
            .cloned()
            .map(Value::Str)
            .unwrap_or_else(|| Value::Str(String::new()))
    }

    pub fn set_field(&mut self, i: i32, val: &str) {
        if i < 1 {
            return;
        }
        let idx = (i - 1) as usize;
        if self.fields.len() <= idx {
            self.fields.resize(idx + 1, String::new());
        }
        self.fields[idx] = val.to_string();
        self.rebuild_record();
        let nf = self.fields.len() as f64;
        self.vars.insert("NF".into(), Value::Num(nf));
    }

    fn rebuild_record(&mut self) {
        let ofs = self
            .vars
            .get("OFS")
            .map(|v| v.as_str())
            .unwrap_or_else(|| " ".into());
        self.record = self.fields.join(&ofs);
    }

    pub fn array_get(&self, name: &str, key: &str) -> Value {
        match self.vars.get(name) {
            Some(Value::Array(a)) => a.get(key).cloned().unwrap_or(Value::Str(String::new())),
            _ => Value::Str(String::new()),
        }
    }

    pub fn array_set(&mut self, name: &str, key: String, val: Value) {
        let e = self
            .vars
            .entry(name.to_string())
            .or_insert_with(|| Value::Array(HashMap::new()));
        match e {
            Value::Array(a) => {
                a.insert(key, val);
            }
            _ => {
                *e = Value::Array(HashMap::from([(key, val)]));
            }
        }
    }

    pub fn array_delete(&mut self, name: &str, key: Option<&str>) {
        if let Some(k) = key {
            if let Some(Value::Array(a)) = self.vars.get_mut(name) {
                a.remove(k);
            }
        } else {
            self.vars.remove(name);
        }
    }

    pub fn array_keys(&self, name: &str) -> Vec<String> {
        match self.vars.get(name) {
            Some(Value::Array(a)) => a.keys().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// Populate `arr` with split parts; uses 1-based string keys "1", "2", ...
    pub fn split_into_array(&mut self, arr_name: &str, parts: &[String]) {
        self.array_delete(arr_name, None);
        for (i, p) in parts.iter().enumerate() {
            self.array_set(
                arr_name,
                format!("{}", i + 1),
                Value::Str(p.clone()),
            );
        }
    }
}
