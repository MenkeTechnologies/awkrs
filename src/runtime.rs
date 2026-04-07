use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Value {
    Str(String),
    Num(f64),
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
        }
    }

    pub fn as_number(&self) -> f64 {
        match self {
            Value::Num(n) => *n,
            Value::Str(s) => s.parse().unwrap_or(0.0),
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
            self.fields = line
                .split_whitespace()
                .map(String::from)
                .collect();
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
}
