//! Which fusevm execution tier a program's bytecode actually reaches.
//!
//! Enabling the JIT is not the same as being compiled by it, and the only
//! honest way to tell the two apart is to ask the VM. This module runs a
//! program and then queries fusevm's own eligibility and cache predicates —
//! `is_block_eligible`, `block_jit_is_compiled`, `trace_is_compiled`,
//! `find_jit_region` — so the answer comes from the compiler that would have
//! done the work rather than from an assumption about it.
//!
//! `awkrs --tiers -f script.awk input` prints the report.

use std::collections::BTreeMap;

use fusevm::{Chunk, ChunkBuilder, JitCompiler, Op};

/// A loop header — the target of a backward branch — and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loop {
    /// Op index of the loop header the backward branch jumps to.
    pub anchor: usize,
    /// Whether fusevm would accept this loop's body as a trace. Asked of
    /// `is_trace_eligible` with the body's ops — the same predicate the
    /// recorder applies to what it recorded, which for a loop whose body has
    /// no early exit is the same op sequence.
    pub trace_eligible: bool,
    /// Whether a compiled trace is installed for this header after the run.
    pub traced: bool,
    /// Whether the tracing JIT gave up on this header.
    pub blacklisted: bool,
}

/// What the tiers did with one chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkTiers {
    /// Which chunk this is — `main` for a whole awk program.
    pub name: String,
    /// Ops in the compiled chunk.
    pub ops: usize,
    /// Whether every op in the chunk is block-JIT eligible, which is what the
    /// whole-chunk block tier requires.
    pub block_eligible: bool,
    /// Whether the block tier holds compiled native code for this chunk.
    pub block_compiled: bool,
    /// The largest contiguous block-eligible op range, if any is large enough
    /// for fusevm to consider it worth compiling.
    pub largest_eligible_region: Option<(usize, usize)>,
    /// Every loop header, and whether the tracing JIT compiled it.
    pub loops: Vec<Loop>,
    /// Op kinds the **block** tier refuses, by occurrence count — what keeps
    /// the whole chunk from being compiled in one piece.
    ///
    /// Not the same question as whether a loop is traced: the tracing tier
    /// takes `GetVar` / `SetVar` (fusevm promotes a referenced global to a
    /// register at trace entry and spills it at every exit), so a chunk can
    /// list those here and still reach native code through a trace.
    pub ineligible: BTreeMap<String, usize>,
}

impl ChunkTiers {
    /// Whether any tier holds compiled native code for this chunk.
    pub fn reaches_native(&self) -> bool {
        self.block_compiled || self.loops.iter().any(|l| l.traced)
    }
}

/// What the tiers did with one program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Every chunk the program compiled to, in the order they were lowered.
    pub chunks: Vec<ChunkTiers>,
}

impl Report {
    /// Whether any tier holds compiled native code for any of the program's
    /// chunks.
    pub fn reaches_native(&self) -> bool {
        self.chunks.iter().any(|c| c.reaches_native())
    }
}

impl std::fmt::Display for ChunkTiers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "ops                     {}", self.ops)?;
        writeln!(f, "block-JIT eligible      {}", self.block_eligible)?;
        writeln!(f, "block-JIT compiled      {}", self.block_compiled)?;
        match self.largest_eligible_region {
            Some((s, e)) => writeln!(f, "largest eligible region {s}..{e} ({} ops)", e - s)?,
            None => writeln!(f, "largest eligible region none")?,
        }
        if self.loops.is_empty() {
            writeln!(f, "loops                   none")?;
        }
        for l in &self.loops {
            writeln!(
                f,
                "loop @{:<4}             trace-eligible={} traced={} blacklisted={}",
                l.anchor, l.trace_eligible, l.traced, l.blacklisted
            )?;
        }
        if self.ineligible.is_empty() {
            writeln!(f, "block-ineligible ops    none")?;
        } else {
            writeln!(f, "block-ineligible ops")?;
            for (name, count) in &self.ineligible {
                writeln!(f, "  {name:<22}{count}")?;
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A single-chunk program needs no section headers; the fleet's
        // multi-chunk frontends label each one.
        let label = self.chunks.len() > 1;
        for c in &self.chunks {
            if label {
                writeln!(f, "== {} ==", c.name)?;
            }
            write!(f, "{c}")?;
        }
        write!(f, "reaches native code     {}", self.reaches_native())
    }
}

/// Compile `program_text` to its fusevm chunks, run it on the fusevm backend
/// over `files`, and report which tiers took each chunk.
///
/// The program is run because tier membership is a runtime fact: the block tier
/// compiles after its warmup threshold and the tracing tier only after a loop
/// has gone round enough times to be recorded.
///
/// `--tiers` drives the fusevm backend directly rather than honoring
/// `AWKRS_FUSEVM_NATIVE`: asking which fusevm tier took a program is only a
/// meaningful question of the fusevm path, and the `vm.rs` interpreter is not
/// it. A construct the backend does not cover yet is an error here rather than
/// a silent fallback, because a report on a program that never ran on fusevm
/// would be a report about nothing.
pub fn report(
    args: &crate::cli::Args,
    program_text: &str,
    files: &[std::path::PathBuf],
) -> crate::error::Result<Report> {
    let prog = crate::parser::parse_program(program_text)?;
    let named = crate::fusevm_compile::program_chunks(&prog)?;
    if !crate::try_native_run(args, program_text, files)? {
        return Err(crate::error::Error::Runtime(
            "--tiers: this program is outside the fusevm backend's coverage, so no tier ran it"
                .to_string(),
        ));
    }
    Ok(inspect_all(&named))
}


/// Report on every chunk a compiled program holds, in lowering order.
pub fn inspect_all(named: &[(String, Chunk)]) -> Report {
    Report {
        chunks: named.iter().map(|(n, c)| inspect_chunk(n, c)).collect(),
    }
}

/// Report on one already-executed chunk.
pub fn inspect_chunk(name: &str, chunk: &Chunk) -> ChunkTiers {
    let jit = JitCompiler::new();
    let loops = loop_anchors(&chunk.ops)
        .into_iter()
        .map(|anchor| Loop {
            anchor,
            trace_eligible: body_of(&chunk.ops, anchor)
                .is_some_and(|body| jit.is_trace_eligible(body, anchor)),
            traced: jit.trace_is_compiled(chunk, anchor),
            blacklisted: jit.trace_is_blacklisted(chunk, anchor),
        })
        .collect();

    let mut ineligible: BTreeMap<String, usize> = BTreeMap::new();
    for op in &chunk.ops {
        if !op_is_eligible(&jit, op) {
            *ineligible.entry(op_name(op)).or_default() += 1;
        }
    }

    ChunkTiers {
        name: name.to_string(),
        ops: chunk.ops.len(),
        block_eligible: jit.is_block_eligible(chunk),
        block_compiled: jit.block_jit_is_compiled(chunk),
        largest_eligible_region: jit.find_jit_region(chunk),
        loops,
        ineligible,
    }
}

/// Every op index a backward branch jumps to — fusevm anchors a trace at each.
fn loop_anchors(ops: &[Op]) -> Vec<usize> {
    let mut anchors: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter_map(|(ip, op)| match op {
            Op::Jump(t)
            | Op::JumpIfTrue(t)
            | Op::JumpIfFalse(t)
            | Op::JumpIfTrueKeep(t)
            | Op::JumpIfFalseKeep(t)
                if *t <= ip =>
            {
                Some(*t)
            }
            _ => None,
        })
        .collect();
    anchors.sort_unstable();
    anchors.dedup();
    anchors
}

/// The op sequence one iteration of the loop at `anchor` runs: from the header
/// through the backward branch that closes it. `None` when nothing closes it.
fn body_of(ops: &[Op], anchor: usize) -> Option<&[Op]> {
    let close = ops.iter().enumerate().position(|(ip, op)| {
        ip >= anchor
            && matches!(
                op,
                Op::Jump(t) | Op::JumpIfTrue(t) | Op::JumpIfFalse(t)
                    if *t == anchor
            )
    })?;
    Some(&ops[anchor..=close])
}

/// Whether fusevm's block tier accepts this op, asked by handing the JIT a
/// chunk holding just that op. Whole-chunk eligibility is the conjunction of
/// the per-op decision, so a one-op chunk isolates it.
fn op_is_eligible(jit: &JitCompiler, op: &Op) -> bool {
    let mut b = ChunkBuilder::new();
    b.emit(op.clone(), 1);
    jit.is_block_eligible(&b.build())
}

/// An op's variant name, without its operands, so occurrences group.
fn op_name(op: &Op) -> String {
    let text = format!("{op:?}");
    match text.split_once('(') {
        Some((name, _)) => name.to_string(),
        None => text,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The report can say yes. A counted loop built by hand in the rotated
    /// shape — entered at its body, closed by a conditional backward branch —
    /// is the one shape fusevm's trace compiler accepts (`jit.rs` compiles a
    /// `JumpIfTrue`/`JumpIfFalse` close and bails on an unconditional `Jump`),
    /// and after a run the report finds the installed trace. Without this, the
    /// refusals the awk chunks report could be a report that only ever says no.
    #[test]
    fn a_rotated_slot_loop_reaches_a_compiled_trace() {
        let mut b = ChunkBuilder::new();
        b.emit(Op::LoadInt(0), 1);
        b.emit(Op::SetSlot(0), 1);
        let enter = b.emit(Op::Jump(usize::MAX), 1);
        let body = b.current_pos();
        b.emit(Op::GetSlot(0), 1);
        b.emit(Op::LoadInt(1), 1);
        b.emit(Op::Add, 1);
        b.emit(Op::SetSlot(0), 1);
        let cond = b.current_pos();
        b.patch_jump(enter, cond);
        b.emit(Op::GetSlot(0), 1);
        b.emit(Op::LoadInt(200_000), 1);
        b.emit(Op::NumLt, 1);
        b.emit(Op::JumpIfTrue(body), 1);
        b.emit(Op::GetSlot(0), 1);
        let chunk = b.build();

        let mut vm = fusevm::VM::new(chunk.clone());
        vm.enable_tracing_jit();
        vm.run();

        let report = inspect_all(&[("main".to_string(), chunk)]);
        assert_eq!(report.chunks[0].loops.len(), 1, "{report}");
        assert!(report.chunks[0].loops[0].traced, "{report}");
        assert!(report.reaches_native(), "{report}");
    }

    /// An awk program is reported one chunk per rule: `BEGIN`, each per-record
    /// rule, and `END` are compiled and driven separately, so the report has to
    /// name all of them. This pins that split — and that the counted loop is
    /// found in the `BEGIN` chunk where it was written.
    #[test]
    fn each_rule_is_its_own_chunk_and_the_loop_is_found_in_begin() {
        let prog = crate::parser::parse_program(
            "BEGIN { t = 0; for (i = 0; i < 10; i++) t += i } { n++ } END { print n }",
        )
        .expect("parses");
        let named = crate::fusevm_compile::program_chunks(&prog).expect("compiles");
        let names: Vec<&str> = named.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["BEGIN[0]", "rule[0]", "END[0]"], "{names:?}");

        let report = inspect_all(&named);
        assert_eq!(report.chunks[0].loops.len(), 1, "{report}");
        assert!(report.chunks[1].loops.is_empty(), "{report}");
        assert!(report.chunks[2].loops.is_empty(), "{report}");
    }

    /// awk's scalars are host-backed (`AwkSpecialGet`/`AwkSpecialSet`) so they
    /// survive the BEGIN / per-record / END chunk boundaries. That is exactly
    /// what keeps a counted loop out of both tiers, and the report names the
    /// ops responsible — the list to shrink to reach native code.
    #[test]
    fn host_backed_scalars_are_what_keep_the_loop_out_of_both_tiers() {
        let prog = crate::parser::parse_program("BEGIN { for (i = 0; i < 10; i++) t += i }")
            .expect("parses");
        let report = inspect_all(&crate::fusevm_compile::program_chunks(&prog).expect("compiles"));
        let begin = &report.chunks[0];
        assert!(!begin.loops[0].trace_eligible, "{report}");
        assert!(!begin.block_eligible, "{report}");
        assert!(
            begin.ineligible.contains_key("AwkSpecialGet")
                && begin.ineligible.contains_key("AwkSpecialSet"),
            "the host-backed scalar ops are what refuse: {report}"
        );
        assert!(!report.reaches_native(), "{report}");
    }
}
