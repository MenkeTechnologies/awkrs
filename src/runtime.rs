use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::rc::Rc;

use crate::error::{Error, Result};

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

    /// POSIX-style: true if the value is numeric (including string that looks like number).
    pub fn is_numeric_str(&self) -> bool {
        match self {
            Value::Num(_) => true,
            Value::Str(s) => {
                let t = s.trim();
                !t.is_empty() && t.parse::<f64>().is_ok()
            }
            Value::Array(_) => false,
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
    /// Set by `exit`; END rules run before process exit (POSIX).
    pub exit_pending: bool,
    pub exit_code: i32,
    /// Primary input stream for `getline` without `< file` (same as main record loop).
    pub input_reader: Option<Rc<RefCell<BufReader<Box<dyn Read>>>>>,
    /// Open files for `getline < path` / `close`.
    pub file_handles: HashMap<String, BufReader<File>>,
    pub rand_seed: u64,
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
            exit_pending: false,
            exit_code: 0,
            input_reader: None,
            file_handles: HashMap::new(),
            rand_seed: 1,
        }
    }

    pub fn attach_input_reader(&mut self, r: Rc<RefCell<BufReader<Box<dyn Read>>>>) {
        self.input_reader = Some(r);
    }

    pub fn detach_input_reader(&mut self) {
        self.input_reader = None;
    }

    /// Next line from the primary input stream (used by `getline` with no redirection).
    pub fn read_line_primary(&mut self) -> Result<Option<String>> {
        let Some(r) = &self.input_reader else {
            return Err(Error::Runtime(
                "`getline` with no file is only valid during normal input".into(),
            ));
        };
        let mut line = String::new();
        let n = r.borrow_mut().read_line(&mut line).map_err(Error::Io)?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(line))
    }

    /// `getline var < filename` — one line from a kept-open file handle.
    pub fn read_line_file(&mut self, path: &str) -> Result<Option<String>> {
        let p = Path::new(path);
        if !self.file_handles.contains_key(path) {
            let f = File::open(p).map_err(|e| Error::Runtime(format!("open {path}: {e}")))?;
            self.file_handles
                .insert(path.to_string(), BufReader::new(f));
        }
        let reader = self.file_handles.get_mut(path).unwrap();
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(Error::Io)?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(line))
    }

    pub fn close_handle(&mut self, path: &str) -> f64 {
        let _ = self.file_handles.remove(path);
        0.0
    }

    pub fn rand(&mut self) -> f64 {
        self.rand_seed = self.rand_seed.wrapping_mul(1103515245).wrapping_add(12345);
        f64::from((self.rand_seed >> 16) as u32 & 0x7fff) / 32768.0
    }

    pub fn srand(&mut self, n: Option<u32>) -> f64 {
        let prev = self.rand_seed;
        self.rand_seed = n.map(|x| x as u64).unwrap_or(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() ^ (d.subsec_nanos() as u64))
                .unwrap_or(1),
        );
        (prev & 0xffff_ffff) as f64
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

    pub fn set_record_from_line(&mut self, line: &str) {
        let fs = self
            .vars
            .get("FS")
            .map(|v| v.as_str())
            .unwrap_or_else(|| " ".into());
        self.set_field_sep_split(&fs, line.trim_end_matches(['\n', '\r']));
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

    pub fn split_into_array(&mut self, arr_name: &str, parts: &[String]) {
        self.array_delete(arr_name, None);
        for (i, p) in parts.iter().enumerate() {
            self.array_set(arr_name, format!("{}", i + 1), Value::Str(p.clone()));
        }
    }
}
