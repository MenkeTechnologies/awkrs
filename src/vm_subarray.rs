// gawk arrays of arrays for the awkrs VM: `a[i][j]`, `k in a[i]`,
// `delete a[i][j]`, `for (k in a[i])`, and subarrays passed to user functions.
// Child module of `vm` via `#[path]`; `use super::*` resolves to vm.rs's items.
//
// An element of an awk array is a `Value`, so a subarray is simply an element
// holding `Value::Array`. Every operation names its target as the root array's
// name plus the subscripts leading down to it; `subarray_mut` walks them,
// creating a subarray where an element is missing — gawk does the same, for a
// read (`x = a[1][2]`) as much as a store — and refusing a scalar on the way
// with gawk's fatal.

use super::*;

/// Marker a user-function call pushes in place of a variable name for an
/// argument that is an array element: the argument's value is then either the
/// element itself (a scalar) or an element reference (see [`elem_ref_value`]),
/// so a subarray passed by reference can be written back after the call.
pub(crate) const ELEM_REF_MARKER: &str = "\0elem";

/// gawk's spelling of an element in a fatal: `a["1"]["x"]`.
fn elem_display(name: &str, keys: &[Vec<u8>]) -> String {
    let mut s = name.to_string();
    for k in keys {
        s.push_str("[\"");
        s.push_str(&String::from_utf8_lossy(k));
        s.push_str("\"]");
    }
    s
}

fn scalar_as_array(name: &str, keys: &[Vec<u8>]) -> Error {
    Error::Runtime(format!(
        "attempt to use scalar `{}' as an array",
        elem_display(name, keys)
    ))
}

/// gawk's fatal for a subarray read where a scalar is needed.
fn array_in_scalar_context(name: &str, keys: &[Vec<u8>]) -> Error {
    Error::Runtime(format!(
        "attempt to use array `{}' in a scalar context",
        elem_display(name, keys)
    ))
}

/// `slot` as an array: an unassigned one becomes an empty array.
fn as_array_mut<'s>(slot: &'s mut Value, name: &str) -> Result<&'s mut AwkArray> {
    if matches!(slot, Value::Uninit) {
        *slot = Value::Array(AwkArray::new());
    }
    match slot {
        Value::Array(a) => Ok(a),
        _ => Err(scalar_as_array(name, &[])),
    }
}

impl VmCtx<'_> {
    /// Pop `n` subscripts (pushed outermost first) as array keys.
    pub(super) fn pop_subscripts(&mut self, n: usize) -> Vec<Vec<u8>> {
        let start = self.stack.len().saturating_sub(n);
        let vals: Vec<Value> = self.stack.drain(start..).collect();
        vals.iter()
            .map(|v| {
                let mut kbuf = crate::runtime::KeyBuf::new();
                self.rt.array_key_bytes_in(v, &mut kbuf).into_owned()
            })
            .collect()
    }

    /// The array `name` itself: a function's array parameter or local when the
    /// innermost frame binding the name has one, else the global — created
    /// when unassigned.
    fn root_array_mut(&mut self, name: &str) -> Result<&mut AwkArray> {
        if name == "SYMTAB" {
            return Err(Error::Runtime(
                "SYMTAB elements cannot be used as subarrays".into(),
            ));
        }
        if let Some(i) = self.locals.iter().rposition(|f| f.contains_key(name)) {
            let slot = self.locals[i].get_mut(name).expect("frame binds name");
            return as_array_mut(slot, name);
        }
        if !self.rt.vars.contains_key(name) {
            // A parallel worker reads the shared globals; it writes a copy.
            let inherited = match self.rt.get_global_var(name) {
                Some(Value::Array(a)) => Value::Array(a.clone()),
                _ => Value::Uninit,
            };
            self.rt.vars.insert(name.to_string(), inherited);
        }
        as_array_mut(self.rt.vars.get_mut(name).expect("inserted"), name)
    }

    /// The subarray `name[path[0]]..[path[n-1]]`. A missing element on the way
    /// becomes an empty subarray; a scalar one (even unassigned) is gawk's
    /// "attempt to use scalar … as an array".
    pub(super) fn subarray_mut(&mut self, name: &str, path: &[Vec<u8>]) -> Result<&mut AwkArray> {
        let mut arr = self.root_array_mut(name)?;
        for (i, k) in path.iter().enumerate() {
            if !arr.contains_key_bytes(k) {
                arr.insert_bytes(k, Value::Array(AwkArray::new()));
            }
            match arr.get_mut_bytes(k) {
                Some(Value::Array(sub)) => arr = sub,
                _ => return Err(scalar_as_array(name, &path[..=i])),
            }
        }
        Ok(arr)
    }

    /// `name[key]` whatever it holds (a subarray included), created
    /// unassigned when missing — `elem_any` without a subarray path, through
    /// the same frame-aware lookup as a plain `a[k]` read.
    pub(super) fn elem_any_flat(&mut self, name: &str, key: &[u8]) -> Result<Value> {
        check_array_target(self, name)?;
        Ok(self.array_elem_get_vivify_bytes(name, key))
    }

    /// `name[path..][key]` whatever it holds, created unassigned when missing.
    pub(super) fn elem_any(&mut self, name: &str, path: &[Vec<u8>], key: &[u8]) -> Result<Value> {
        let arr = self.subarray_mut(name, path)?;
        if let Some(v) = arr.get_bytes(key) {
            return Ok(v.clone());
        }
        arr.insert_bytes(key, Value::Uninit);
        Ok(Value::Uninit)
    }

    /// `name[path..][key]` as a scalar: a subarray there is a fatal.
    pub(super) fn sub_get(&mut self, name: &str, path: &[Vec<u8>], key: &[u8]) -> Result<Value> {
        let v = self.elem_any(name, path, key)?;
        if matches!(v, Value::Array(_)) {
            let mut keys = path.to_vec();
            keys.push(key.to_vec());
            return Err(array_in_scalar_context(name, &keys));
        }
        Ok(v)
    }

    /// `name[path..][key] = val`. Replacing a subarray with a scalar is a fatal.
    pub(super) fn sub_set(
        &mut self,
        name: &str,
        path: &[Vec<u8>],
        key: &[u8],
        val: Value,
    ) -> Result<()> {
        let arr = self.subarray_mut(name, path)?;
        if let Some(Value::Array(_)) = arr.get_bytes(key) {
            let mut keys = path.to_vec();
            keys.push(key.to_vec());
            return Err(array_in_scalar_context(name, &keys));
        }
        arr.insert_bytes(key, val);
        Ok(())
    }

    /// The element a user-function argument `name[k1]..[kn]` names, ready to
    /// pass: a scalar is passed as its value; a subarray, or an element that
    /// does not exist yet (the callee may make it a subarray), is passed as a
    /// reference that the call resolves and writes back.
    pub(super) fn elem_ref(&mut self, name: &str, keys: Vec<Vec<u8>>) -> Result<Value> {
        let (key, path) = keys.split_last().expect("at least one subscript");
        match self.elem_any(name, path, key)? {
            Value::Array(_) | Value::Uninit => Ok(elem_ref_value(name, &keys)),
            scalar => Ok(scalar),
        }
    }

    /// The keys of `for (k in name[path..])`, in `PROCINFO["sorted_in"]` order.
    pub(super) fn sub_for_in_keys(&mut self, name: &str, path: &[Vec<u8>]) -> Result<Vec<AwkStr>> {
        // A snapshot: the loop body may change the subarray, and a user
        // comparison function runs awk code that may too.
        let sub = self.subarray_mut(name, path)?.clone();
        if let SortedInMode::CustomFn(fname) = sorted_in_mode(self.rt) {
            let mut keys = sub.keys();
            if !self.rt.posix {
                sort_keys_by_user_fn(self, &mut keys, &fname, |_, k| {
                    sub.get_bytes(k.as_bytes())
                        .cloned()
                        .unwrap_or(Value::Uninit)
                })?;
            }
            return Ok(keys);
        }
        Ok(self.rt.for_in_keys_of(&sub))
    }

    /// Pop an element's subscripts: `depth` path keys under the final key.
    pub(super) fn pop_elem(&mut self, depth: u16) -> (Vec<Vec<u8>>, Vec<u8>) {
        let mut keys = self.pop_subscripts(depth as usize + 1);
        let key = keys.pop().unwrap_or_default();
        (keys, key)
    }

    /// [`Op::ElemBind`]: copy `name[keys..]` into the hidden variable `tmp`.
    pub(super) fn elem_bind(
        &mut self,
        name: String,
        keys: Vec<Vec<u8>>,
        tmp: u32,
        array: bool,
    ) -> Result<()> {
        let v = if array {
            Value::Array(self.subarray_mut(&name, &keys)?.clone())
        } else {
            let (key, path) = keys.split_last().expect("at least one subscript");
            self.sub_get(&name, path, key)?
        };
        let tmp_name = self.str_ref(tmp).to_string();
        self.rt.vars.insert(tmp_name, v);
        self.elem_binds.push((name, keys, tmp));
        Ok(())
    }

    /// [`Op::ElemUnbind`]: store the hidden variable back where it came from.
    pub(super) fn elem_unbind(&mut self, tmp: u32) -> Result<()> {
        let Some((name, keys, bound)) = self.elem_binds.pop() else {
            return Ok(());
        };
        debug_assert_eq!(bound, tmp);
        let tmp_name = self.str_ref(tmp).to_string();
        let v = self.rt.vars.remove(&tmp_name).unwrap_or(Value::Uninit);
        let (key, path) = keys.split_last().expect("at least one subscript");
        if path.is_empty() {
            self.array_elem_set_bytes(&name, key, v);
        } else {
            self.subarray_mut(&name, path)?.insert_bytes(key, v);
        }
        Ok(())
    }
}

/// An element reference: an array holding the root array's name at `0` and
/// the subscripts at `1..`. Only ever found at an argument position marked
/// [`ELEM_REF_MARKER`], where a real array value cannot occur (an element that
/// holds a subarray is always passed this way).
pub(super) fn elem_ref_value(name: &str, keys: &[Vec<u8>]) -> Value {
    let mut r = AwkArray::new();
    r.insert_int(0, Value::Str(AwkStr::from(name)));
    for (i, k) in keys.iter().enumerate() {
        r.insert_int(i as i64 + 1, Value::Str(AwkStr::from(&k[..])));
    }
    Value::Array(r)
}

/// The root name and subscripts of an element reference.
pub(super) fn decode_elem_ref(r: &AwkArray) -> (String, Vec<Vec<u8>>) {
    let name = r
        .get_int(0)
        .map(|v| v.as_str().to_string())
        .unwrap_or_default();
    let keys = (1..r.len() as i64)
        .filter_map(|i| r.get_int(i))
        .map(|v| match v {
            Value::Str(s) => s.as_bytes().to_vec(),
            other => other.as_str().into_bytes(),
        })
        .collect();
    (name, keys)
}

/// Run one arrays-of-arrays op. Out of line on purpose: the interpreter loop's
/// stack frame is nested once per awk-level function call, so these arms'
/// locals must not grow it.
#[inline(never)]
pub(super) fn exec_subarray_op(ctx: &mut VmCtx<'_>, op: &Op) -> Result<()> {
    match *op {
        Op::SubGet(arr, depth) => {
            let (path, key) = ctx.pop_elem(depth);
            let name = ctx.str_ref(arr).to_string();
            let v = ctx.sub_get(&name, &path, &key)?;
            ctx.push(v);
        }
        Op::SubSet(arr, depth) => {
            let val = ctx.pop();
            let (path, key) = ctx.pop_elem(depth);
            let name = ctx.str_ref(arr).to_string();
            ctx.sub_set(&name, &path, &key, val.clone())?;
            ctx.push(val);
        }
        Op::SubCompound(arr, depth, bop) => {
            let rhs = ctx.pop();
            let (path, key) = ctx.pop_elem(depth);
            let name = ctx.str_ref(arr).to_string();
            let old = ctx.sub_get(&name, &path, &key)?;
            let new_val = apply_binop(bop, &old, &rhs, ctx.rt.bignum, ctx.rt)?;
            ctx.sub_set(&name, &path, &key, new_val.clone())?;
            ctx.push(new_val);
        }
        Op::SubIncDec(arr, depth, kind) => {
            let (path, key) = ctx.pop_elem(depth);
            let name = ctx.str_ref(arr).to_string();
            let old = ctx.sub_get(&name, &path, &key)?;
            let delta = incdec_delta(kind);
            let (new_val, ret) = if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let old_f = value_to_float(&old, prec, round);
                let d = Float::with_val(prec, delta);
                let new_f = Float::with_val_round(prec, &old_f + &d, round).0;
                let ret = match kind {
                    IncDecOp::PreInc | IncDecOp::PreDec => Value::Mpfr(new_f.clone()),
                    IncDecOp::PostInc | IncDecOp::PostDec => Value::Mpfr(old_f),
                };
                (Value::Mpfr(new_f), ret)
            } else {
                let old_n = old.as_number();
                let new_n = old_n + delta;
                (
                    Value::Num(new_n),
                    Value::Num(incdec_push(kind, old_n, new_n)),
                )
            };
            ctx.sub_set(&name, &path, &key, new_val)?;
            ctx.push(ret);
        }
        Op::SubIn(arr, depth) => {
            let (path, key) = ctx.pop_elem(depth);
            let name = ctx.str_ref(arr).to_string();
            let b = ctx.subarray_mut(&name, &path)?.contains_key_bytes(&key);
            ctx.push(Value::Num(if b { 1.0 } else { 0.0 }));
        }
        Op::SubDelete(arr, depth) => {
            let (path, key) = ctx.pop_elem(depth);
            let name = ctx.str_ref(arr).to_string();
            ctx.subarray_mut(&name, &path)?.remove_bytes(&key);
        }
        Op::SubForInStart(arr, depth) => {
            let path = ctx.pop_subscripts(depth as usize);
            let name = ctx.str_ref(arr).to_string();
            let keys = ctx.sub_for_in_keys(&name, &path)?;
            ctx.for_in_iters.push(ForInState { keys, index: 0 });
        }
        Op::ElemAny(arr, depth) => {
            let (path, key) = ctx.pop_elem(depth - 1);
            let name = ctx.str_ref(arr).to_string();
            let v = if path.is_empty() {
                ctx.elem_any_flat(&name, &key)?
            } else {
                ctx.elem_any(&name, &path, &key)?
            };
            ctx.push(v);
        }
        Op::ElemRef(arr, depth) => {
            let keys = ctx.pop_subscripts(depth as usize);
            let name = ctx.str_ref(arr).to_string();
            let v = ctx.elem_ref(&name, keys)?;
            ctx.push(v);
        }
        Op::ElemBind {
            arr,
            depth,
            tmp,
            array,
        } => {
            let keys = ctx.pop_subscripts(depth as usize);
            let name = ctx.str_ref(arr).to_string();
            ctx.elem_bind(name, keys, tmp, array)?;
        }
        Op::ElemUnbind(tmp) => ctx.elem_unbind(tmp)?,
        _ => unreachable!("not an arrays-of-arrays op: {op:?}"),
    }
    Ok(())
}

/// `a[k]` read as a scalar where `a[k]` holds a subarray.
#[cold]
#[inline(never)]
pub(super) fn subarray_read_fatal(ctx: &VmCtx<'_>, name: &str, key: &Value) -> Error {
    let key = ctx.rt.value_to_array_key(key).into_bytes();
    array_in_scalar_context(name, &[key])
}

/// `a[k] = scalar` replaced the subarray `sub`: put it back and report.
#[cold]
#[inline(never)]
pub(super) fn subarray_overwrite_fatal(
    ctx: &mut VmCtx<'_>,
    name: &str,
    key: &Value,
    sub: AwkArray,
) -> Error {
    let key = ctx.rt.value_to_array_key(key).into_bytes();
    ctx.array_elem_set_bytes(name, &key, Value::Array(sub));
    array_in_scalar_context(name, &[key])
}
