//! Experimental Cranelift JIT for **numeric stack-machine** bytecode.
//!
//! This compiles a linear subset of [`crate::bytecode::Op`] (`PushNum`, `Add`, `Sub`, `Mul`, `Div`,
//! `Neg`, `Pop`) into a native `extern "C" fn() -> f64` with no interpreter overhead. It is the
//! foundation for future integration with the full VM (pattern actions, slots, calls to Rust
//! helpers, etc.).
//!
//! Enable with `AWKRS_JIT=1` and use [`try_jit_dispatch_numeric_chunk`] from the VM when every
//! instruction in the chunk is supported; otherwise fall back to [`crate::vm::execute`].

use crate::bytecode::Op;
use cranelift_codegen::ir::{types, AbiParam, InstBuilder, UserFuncName};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};
use std::mem;

/// Holds generated machine code; keep this alive while calling [`JitNumericChunk::call_f64`].
pub struct JitNumericChunk {
    module: JITModule,
    func_id: FuncId,
}

impl JitNumericChunk {
    /// Run the compiled expression; returns the single `f64` left on the conceptual stack.
    pub fn call_f64(&self) -> f64 {
        let ptr = self.module.get_finalized_function(self.func_id);
        unsafe {
            let f: extern "C" fn() -> f64 = mem::transmute(ptr);
            f()
        }
    }
}

/// True if `ops` is a straight-line numeric expression ending with exactly one value.
pub fn is_numeric_stack_eligible(ops: &[Op]) -> bool {
    let mut depth: i32 = 0;
    for op in ops {
        match op {
            Op::PushNum(_) => depth += 1,
            Op::Add | Op::Sub | Op::Mul | Op::Div => {
                if depth < 2 {
                    return false;
                }
                depth -= 1;
            }
            Op::Neg => {
                if depth < 1 {
                    return false;
                }
            }
            Op::Pop => {
                if depth < 1 {
                    return false;
                }
                depth -= 1;
            }
            _ => return false,
        }
    }
    depth == 1
}

fn new_jit_module() -> Option<JITModule> {
    let mut flag_builder = settings::builder();
    flag_builder.set("use_colocated_libcalls", "false").unwrap();
    flag_builder.set("is_pic", "false").unwrap();
    let isa_builder = cranelift_native::builder().ok()?;
    let isa = isa_builder.finish(settings::Flags::new(flag_builder)).ok()?;
    let builder = JITBuilder::with_isa(isa, default_libcall_names());
    Some(JITModule::new(builder))
}

/// Compile `ops` to native code, or `None` if unsupported or Cranelift fails.
pub fn try_compile_numeric_expr(ops: &[Op]) -> Option<JitNumericChunk> {
    if !is_numeric_stack_eligible(ops) {
        return None;
    }
    let mut module = new_jit_module()?;
    let mut ctx = module.make_context();
    let mut func_ctx = FunctionBuilderContext::new();

    let mut sig = module.make_signature();
    sig.returns.push(AbiParam::new(types::F64));

    let func_id = module
        .declare_function("awkrs_jit_numeric", Linkage::Export, &sig)
        .ok()?;

    ctx.func.signature = sig;
    ctx.func.name = UserFuncName::user(0, func_id.as_u32());

    {
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let mut stack: Vec<cranelift_codegen::ir::Value> = Vec::new();
        for op in ops {
            match op {
                Op::PushNum(n) => {
                    let v = builder.ins().f64const(*n);
                    stack.push(v);
                }
                Op::Add => {
                    let b = stack.pop().expect("validated");
                    let a = stack.pop().expect("validated");
                    stack.push(builder.ins().fadd(a, b));
                }
                Op::Sub => {
                    let b = stack.pop().expect("validated");
                    let a = stack.pop().expect("validated");
                    stack.push(builder.ins().fsub(a, b));
                }
                Op::Mul => {
                    let b = stack.pop().expect("validated");
                    let a = stack.pop().expect("validated");
                    stack.push(builder.ins().fmul(a, b));
                }
                Op::Div => {
                    let b = stack.pop().expect("validated");
                    let a = stack.pop().expect("validated");
                    stack.push(builder.ins().fdiv(a, b));
                }
                Op::Neg => {
                    let a = stack.pop().expect("validated");
                    stack.push(builder.ins().fneg(a));
                }
                Op::Pop => {
                    stack.pop().expect("validated");
                }
                _ => unreachable!("filtered by is_numeric_stack_eligible"),
            }
        }
        let res = stack.pop().expect("depth 1");
        builder.ins().return_(&[res]);
        builder.seal_all_blocks();
        builder.finalize();
    }

    module.define_function(func_id, &mut ctx).ok()?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().ok()?;

    Some(JitNumericChunk { module, func_id })
}

/// If `AWKRS_JIT` is set to `1` and the chunk is eligible, run the JIT result and return `Some(f64)`.
/// Otherwise return `None` (caller should interpret the chunk).
pub fn try_jit_dispatch_numeric_chunk(ops: &[Op]) -> Option<f64> {
    if std::env::var_os("AWKRS_JIT").as_deref() != Some("1".as_ref()) {
        return None;
    }
    let jit = try_compile_numeric_expr(ops)?;
    Some(jit.call_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jit_adds_constants() {
        let ops = [Op::PushNum(3.0), Op::PushNum(4.0), Op::Add];
        let j = try_compile_numeric_expr(&ops).expect("compile");
        assert!((j.call_f64() - 7.0).abs() < 1e-15);
    }

    #[test]
    fn jit_complex_expr() {
        // (10 - 2) * 3 / 4 = 6
        let ops = [
            Op::PushNum(10.0),
            Op::PushNum(2.0),
            Op::Sub,
            Op::PushNum(3.0),
            Op::Mul,
            Op::PushNum(4.0),
            Op::Div,
        ];
        let j = try_compile_numeric_expr(&ops).expect("compile");
        assert!((j.call_f64() - 6.0).abs() < 1e-15);
    }

    #[test]
    fn jit_rejects_non_numeric_ops() {
        let ops = [Op::PushNum(1.0), Op::PushStr(0)];
        assert!(try_compile_numeric_expr(&ops).is_none());
    }
}
