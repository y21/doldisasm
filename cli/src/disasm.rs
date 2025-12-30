use anyhow::Context;
use arrayvec::ArrayVec;
use bumpalo::Bump;
use dataflow::{Dataflow, Predecessors, SuccessorTarget, Successors};
use dol::Dol;
use ppc32::{
    Instruction,
    instruction::{BranchOptions, Gpr, Immediate, RegisterVisitor, Spr, compute_branch_target},
};
use std::{array, collections::BTreeMap, iter, ops::Deref, panic::Location};
use std::{fmt::Write, marker::PhantomData};
use typed_index_collections::{TiSlice, TiVec};

use crate::{
    args::{AddrRange, DisassemblyLanguage},
    decoder::{Address, Decoder},
    value::{Parameter, Value, ValueInner},
};

pub fn disasm(dol: &Dol, range: AddrRange, lang: DisassemblyLanguage) -> anyhow::Result<()> {
    let buffer = dol
        .slice_from_load_addr(range.0)
        .context("address is not in any section")?;

    let mut decoder = Decoder::new(buffer, range);

    match lang {
        DisassemblyLanguage::Asm => disasm_asm(&mut decoder)?,
        DisassemblyLanguage::C => disasm_c(&mut decoder)?,
    }

    Ok(())
}

/// Disassemble as assembly code.
fn disasm_asm(decoder: &mut Decoder<'_>) -> anyhow::Result<()> {
    loop {
        match decoder.next_instruction_with_offset() {
            Ok(Some((off, ins))) => println!("{off} {ins:?}"),
            Ok(None) => break,
            Err(err) => {
                eprintln!("(stopping due to decoder error: {err:#x?})");
                break;
            }
        }
    }

    Ok(())
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct InstId(u32);
impl Into<usize> for InstId {
    fn into(self) -> usize {
        self.0 as usize
    }
}
impl From<usize> for InstId {
    fn from(value: usize) -> Self {
        Self(value as u32)
    }
}

type Instructions = TiVec<InstId, (Address, Instruction)>;
type InstructionsDeref = <Instructions as Deref>::Target;

fn ti_iter<K, V>(ti: &TiSlice<K, V>) -> impl Iterator<Item = (K, &V)>
where
    K: From<usize>,
{
    ti.iter().enumerate().map(|(i, v)| (K::from(i), v))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegisterState<'bump> {
    value_read: bool,
    value: Value<'bump>,
}

impl<'bump> RegisterState<'bump> {
    fn join(self, other: Self) -> Self {
        Self {
            value_read: self.value_read || other.value_read,
            value: self.value.join(other.value),
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
struct ConditionRegisterFieldBits<'bump> {
    lt: Value<'bump>,
    gt: Value<'bump>,
    eq: Value<'bump>,
    so: Value<'bump>,
}

#[derive(Debug, Default, Clone, PartialEq)]
struct XerValues<'bump> {
    so: Value<'bump>,
    ov: Value<'bump>,
    ca: Value<'bump>,
}

#[derive(Debug, Default, Clone, PartialEq)]
struct SprValues<'bump> {
    lr: Value<'bump>,
    ctr: Value<'bump>,
    xer: XerValues<'bump>,
    msr: Value<'bump>,
    cr: [ConditionRegisterFieldBits<'bump>; 8],
}

// TODO: idea for later: Cow<> a bunch of things (bump allocated), and have a &'static version for default and uninitialized things
#[derive(Debug, Clone, PartialEq)]
struct BlockState<'bump> {
    gprs: [RegisterState<'bump>; 32],
    sprs: SprValues<'bump>,
    memory: BTreeMap<Value<'bump>, Value<'bump>>,
    /// Whether this block is "diverging", i.e. execution of this function stops here (e.g. due to an unconditional branch or a trap),
    /// in which case successor blocks are not executed and states need not be joined.
    diverging: bool,
}

impl<'bump> Default for BlockState<'bump> {
    fn default() -> Self {
        let mut gprs = [RegisterState {
            value_read: false,
            value: Value::UNINIT,
        }; 32];

        gprs[1].value = Value::CALLER_STACK;
        for p in 0..8 {
            gprs[3 + p].value = Value::parameter(Parameter(p as u8));
        }

        let sprs = SprValues {
            cr: array::from_fn(|_| ConditionRegisterFieldBits {
                lt: Value::UNINIT,
                gt: Value::UNINIT,
                eq: Value::UNINIT,
                so: Value::UNINIT,
            }),
            ctr: Value::UNINIT,
            lr: Value::RETURN_ADDRESS,
            msr: Value::UNINIT,
            xer: XerValues {
                so: Value::UNINIT,
                ov: Value::UNINIT,
                ca: Value::UNINIT,
            },
        };

        Self {
            gprs,
            sprs,
            memory: BTreeMap::new(),
            diverging: false,
        }
    }
}

impl<'bump> BlockState<'bump> {
    pub fn gpr_read(&self, gpr: Gpr) -> bool {
        self.gprs[gpr.0 as usize].value_read
    }

    pub fn set_gpr_read(&mut self, gpr: Gpr, read: bool) {
        self.gprs[gpr.0 as usize].value_read = read;
    }

    pub fn gpr_value(&self, gpr: Gpr) -> Value<'bump> {
        self.gprs[gpr.0 as usize].value
    }

    pub fn set_gpr_value(&mut self, gpr: Gpr, value: Value<'bump>) {
        println!("{:?} = {:?}", gpr, value);
        self.gprs[gpr.0 as usize].value = value;
    }

    fn spr(&self, spr: Spr) -> Value<'bump> {
        match spr {
            Spr::Xer => todo!(),
            Spr::Lr => self.sprs.lr,
            Spr::Ctr => self.sprs.ctr,
            Spr::Msr => self.sprs.msr,
            Spr::Pc => todo!(),
            Spr::Other(_) => todo!(),
        }
    }

    fn set_spr(&mut self, spr: Spr, value: Value<'bump>) {
        println!("{:?} = {:?}", spr, value);
        match spr {
            Spr::Xer => todo!(),
            Spr::Lr => self.sprs.lr = value,
            Spr::Ctr => self.sprs.ctr = value,
            Spr::Msr => self.sprs.msr = value,
            Spr::Pc => todo!(),
            Spr::Other(_) => todo!(),
        }
    }

    fn join(&self, other: &Self) -> Self {
        assert!(!(self.diverging && other.diverging));
        if self.diverging {
            return other.clone();
        }
        if other.diverging {
            return self.clone();
        }
        Self {
            gprs: array::from_fn(|i| self.gprs[i].join(other.gprs[i])),
            sprs: todo!(),
            memory: todo!(),
            diverging: false,
        }
    }

    #[track_caller]
    fn mem_store(&mut self, address: Value<'bump>, value: Value<'bump>) {
        println!("MEM({address:?}) = {value:?}");
        self.memory.insert(address, value);
    }

    fn mem_load_opt(&self, address: Value<'bump>) -> Option<Value<'bump>> {
        self.memory.get(&address).copied()
    }

    #[track_caller]
    fn mem_load(&self, address: Value<'bump>) -> Value<'bump> {
        match self.mem_load_opt(address) {
            Some(val) => val,
            None => {
                panic!("memory read from uninitialized address: {:?}", address)
            }
        }
    }

    fn update_cr_field(&mut self, field: usize, value: Value<'bump>, bump: &'bump Bump) {
        self.sprs.cr[field].eq = value.one_if_zero(bump);
        self.sprs.cr[field].gt = value.one_if_positive(bump);
        self.sprs.cr[field].lt = value.one_if_negative(bump);
        self.sprs.cr[field].so = self.sprs.xer.so;
    }
}

trait GetValue {
    fn value<'bump>(&self, state: &BlockState<'bump>) -> Value<'bump>;
}
impl GetValue for Gpr {
    fn value<'bump>(&self, state: &BlockState<'bump>) -> Value<'bump> {
        state.gpr_value(*self)
    }
}

struct Analysis<'a, 'bump> {
    insts: &'a InstructionsDeref,
    fn_address: u32,
    bump: &'bump Bump,
}

impl<'bump> Dataflow for Analysis<'_, 'bump> {
    type Idx = InstId;
    type BlockState = BlockState<'bump>;
    type BlockItem = Instruction;

    fn compute_preds_and_succs(
        &self,
        preds: &mut Predecessors<Self>,
        succs: &mut Successors<Self>,
    ) {
        let mut store_mapping = |from: InstId, to: SuccessorTarget<_>| {
            if let Some(to) = to.idx() {
                preds.entry(to).or_default().push(from);
            }
            succs.entry(from).or_default().push(to);
        };

        for (idx, &(off, inst)) in ti_iter(&self.insts) {
            if let Instruction::Bc {
                bo: _,
                bi: _,
                target,
                mode,
                link: false,
            } = inst
            {
                if let Some(target) =
                    compute_branch_target(off.0, mode, target).checked_sub(self.fn_address)
                {
                    // If we have a conditional branch to an address before the function itself (i.e. checked_sub = None due to overflow),
                    // then that isn't part of this function and thus not something we need to analyze, hence the checked_sub.
                    // The difference is also in bytes, so the instruction difference is that divided by 4.
                    store_mapping(idx, SuccessorTarget::Id(InstId(target / 4)));
                }

                store_mapping(idx, SuccessorTarget::Id(InstId(idx.0 + 1)));
            } else if let Instruction::Bclr { bo: _, bi: _, link } = inst {
                assert!(!link, "linking bclr not supported yet");
                store_mapping(idx, SuccessorTarget::Return);
            }
        }
    }

    fn initial_idx() -> Self::Idx {
        InstId(0)
    }

    fn join_states(a: &Self::BlockState, b: &Self::BlockState) -> Self::BlockState {
        a.join(b)
    }

    fn iter(&self) -> impl Iterator<Item = (Self::Idx, Self::BlockItem)> {
        ti_iter(&self.insts).map(|(i, &(_, inst))| (i, inst))
    }

    fn iter_block(
        &self,
        InstId(idx): Self::Idx,
    ) -> impl Iterator<Item = (Self::Idx, Self::BlockItem)> {
        self.iter().skip(idx as usize)
    }

    fn apply_effect(&self, state: &mut Self::BlockState, idx: Self::Idx, data: &Self::BlockItem) {
        let inst_addr = self.fn_address + 4 * idx.0;
        // println!("[{:x}] {data:?}", inst_addr);

        data.for_each_read_gpr(|gpr| state.set_gpr_read(gpr, true));

        match *data {
            Instruction::Stwu {
                source,
                dest,
                imm: Immediate(imm),
            } => {
                let src_val = state.gpr_value(source);
                let effective_address = dest.value(state).add(Value::i16(imm), self.bump);

                state.set_gpr_value(dest, effective_address);
                state.mem_store(effective_address, src_val);
            }
            Instruction::Or {
                source,
                dest,
                or_with,
                rc,
            } => {
                let result = if source == or_with {
                    // Simpler mnemonic: oring the source with itself into dest is just a register move from source to dest
                    let src_val = state.gpr_value(source);
                    state.set_gpr_value(dest, src_val);
                    src_val
                } else {
                    let src_val = state.gpr_value(source);
                    let or_with_val = state.gpr_value(or_with);
                    let result = src_val.bit_or(or_with_val, self.bump);
                    state.set_gpr_value(dest, result);
                    result
                };

                if rc {
                    state.update_cr_field(0, result, self.bump);
                }
            }
            Instruction::Mfspr { dest, spr } => {
                let spr_val = state.spr(spr);
                state.set_gpr_value(dest, spr_val);
            }
            Instruction::Addi {
                dest,
                source,
                imm: Immediate(imm),
            } => {
                if source == Gpr::ZERO {
                    state.set_gpr_value(dest, Value::i16(imm));
                } else {
                    let src_val = state.gpr_value(source);
                    let result = src_val.add(Value::i16(imm), self.bump);
                    state.set_gpr_value(dest, result);
                }
            }
            Instruction::Stw {
                source,
                dest,
                imm: Immediate(imm),
            } => {
                let effective_address = dest.value(state).add(Value::i16(imm), self.bump);
                let src_val = state.gpr_value(source);
                state.mem_store(effective_address, src_val);
            }
            Instruction::Branch { target, mode, link } => {
                state.clobber_caller_saved();
                if link {
                    let target = compute_branch_target(inst_addr, mode, target);
                    state.set_gpr_value(Gpr::RETURN, Value::call_result(target));
                }
            }
            Instruction::Bc {
                bo,
                bi,
                target,
                mode,
                link,
            } => {
                // let (div, rem) = (bi as usize / 4, bi as usize % 4);
                // let cr = &state.sprs.cr[div];
                // let condition_value = match rem {
                //     0 => cr.lt,
                //     1 => cr.gt,
                //     2 => cr.eq,
                //     3 => cr.so,
                //     _ => unreachable!(),
                // };
                // dbg!(&condition_value, bo);

                state.clobber_caller_saved();
                if link {
                    let target = compute_branch_target(inst_addr, mode, target);
                    state.set_gpr_value(Gpr::RETURN, Value::call_result(target));
                }
            }
            Instruction::Lwz {
                dest,
                source,
                imm: Immediate(imm),
            } => {
                let effective_address = if source == Gpr::ZERO {
                    Value::i16(imm)
                } else {
                    source.value(state).add(Value::i16(imm), self.bump)
                };
                let loaded_value = state.mem_load(effective_address);
                state.set_gpr_value(dest, loaded_value);
            }
            Instruction::Mtspr { source, spr } => {
                let src_val = state.gpr_value(source);
                state.set_spr(spr, src_val);
            }
            Instruction::Bclr {
                bo: _,
                bi: _,
                link: _,
            } => {
                // End. (For now)
                state.diverging = true;
            }
            _ => todo!(),
        }
    }
}

impl<'bump> BlockState<'bump> {
    fn clobber_caller_saved(&mut self) {
        for gpr in 0..=12 {
            if !matches!(gpr, 1) {
                self.gprs[gpr].value = Value::UNINIT;
                self.gprs[gpr].value_read = false;
            }
        }
        self.sprs = SprValues {
            cr: array::from_fn(|_| ConditionRegisterFieldBits {
                lt: Value::UNINIT,
                gt: Value::UNINIT,
                eq: Value::UNINIT,
                so: Value::UNINIT,
            }),
            ctr: Value::UNINIT,
            lr: Value::UNINIT,
            msr: Value::UNINIT,
            xer: XerValues {
                so: Value::UNINIT,
                ov: Value::UNINIT,
                ca: Value::UNINIT,
            },
        }
    }
}

fn disasm_c(decoder: &mut Decoder<'_>) -> anyhow::Result<()> {
    let fn_address = decoder.address().0;
    let insts: Instructions = iter::from_fn(|| decoder.next_instruction_with_offset().transpose())
        .collect::<Result<_, _>>()
        .map_err(|err| anyhow::anyhow!("decoder error: {err:#x?}"))?;

    let bump = Bump::new();

    let analysis = Analysis {
        insts: &insts,
        fn_address,
        bump: &bump,
    };
    let results = dataflow::run(&analysis);
    println!("==== COMPLETE! ====");

    let mut parameter_gprs = ArrayVec::new();

    // Iterate over the dataflow results to find uses of r3-r10 before they are initialized, which means they are definitely treated
    // as function parameters.
    let final_state = results.for_each_with_input(&analysis, |_, inst, state| {
        struct Visitor<'a, 'bump> {
            state: &'a BlockState<'bump>,
            parameter_gprs: &'a mut ArrayVec<Parameter, 8>,
            inst: Instruction,
        }
        impl RegisterVisitor for Visitor<'_, '_> {
            fn read_gpr(&mut self, gpr: Gpr) {
                let reg_state = &self.state.gprs[gpr.0 as usize];
                if let ValueInner::Param(param) = reg_state.value.inner()
                    && !self.parameter_gprs.contains(&param)
                {
                    // println!(
                    //     "Parameter detected: {param:?} in instruction {:?}",
                    //     self.inst
                    // );
                    self.parameter_gprs.push(param);
                }
            }
            fn write_gpr(&mut self, _gpr: Gpr) {}
        }

        let vis = Visitor {
            state,
            parameter_gprs: &mut parameter_gprs,
            inst,
        };
        inst.visit_registers(vis);
    });
    // Finally, check if r3 has been initialized without being read: this means that there's a return value.
    let ret_reg_state = final_state.gprs[3];
    let has_return = ret_reg_state.value.is_initialized() && !ret_reg_state.value_read;

    // Reconstruct as C code.

    let mut output = String::new();

    if has_return {
        output.push_str("u32 ");
    } else {
        output.push_str("void ");
    }
    write!(output, "{fn_address:#x}(",).unwrap();
    for (i, _gpr) in parameter_gprs.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        write!(output, "u32 p{i}").unwrap();
    }
    write!(output, ") {{\n").unwrap();

    // println!("{output}");

    println!();
    println!(
        "Return value (r3): {:?}",
        final_state.gpr_value(Gpr::RETURN)
    );

    Ok(())
}
