use std::collections::HashMap;

use ppc32::instruction::{Gpr, Register};
use typed_index_collections::TiVec;

use crate::{
    ast::stmt::{VarId, Variable, VariableVisibility},
    flow::ssa::{BlockState, RegisterWithGeneration},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StackRelativeAddress {
    offset: i16,
}

pub struct Variables {
    list: TiVec<VarId, Variable>,
    reg_to_var: HashMap<RegisterWithGeneration, VarId>,
    mem_to_var: HashMap<StackRelativeAddress, VarId>,
}

impl Variables {
    fn new() -> Self {
        Self {
            list: TiVec::new(),
            reg_to_var: HashMap::new(),
            mem_to_var: HashMap::new(),
        }
    }

    pub fn get(&self, var: VarId) -> &Variable {
        &self.list[var]
    }

    #[track_caller]
    pub fn mk_gpr_var(&mut self, gpr: Gpr, state: &BlockState, origin: VarId) -> VarId {
        self.mk_reg_var(
            Register::Gpr(gpr),
            state.registers.gprs[gpr.0 as usize].generation,
            origin,
        )
    }

    #[track_caller]
    pub fn mk_reg_var(&mut self, reg: Register, generation: u32, origin: VarId) -> VarId {
        let key = self.list.push_and_get_key(Variable {
            vis: self.list[origin].vis,
        });
        let reg = RegisterWithGeneration { reg, generation };
        assert!(
            self.reg_to_var.insert(reg, key).is_none(),
            "variable for {:?}_{:?} already exists",
            reg.reg,
            reg.generation
        );
        key
    }

    #[track_caller]
    pub fn mk_root_gpr_var(
        &mut self,
        gpr: Gpr,
        state: &BlockState,
        vis: VariableVisibility,
    ) -> VarId {
        self.mk_root_reg_var(
            Register::Gpr(gpr),
            state.registers.gprs[gpr.0 as usize].generation,
            vis,
        )
    }

    #[track_caller]
    pub fn mk_root_reg_var(
        &mut self,
        reg: Register,
        generation: u32,
        vis: VariableVisibility,
    ) -> VarId {
        let key = self.list.push_and_get_key(Variable { vis });
        let reg = RegisterWithGeneration { reg, generation };
        assert!(self.reg_to_var.insert(reg, key).is_none());
        key
    }

    #[track_caller]
    pub fn id_by_gpr(&mut self, reg: Gpr, state: &BlockState) -> VarId {
        self.id_by_reg(
            Register::Gpr(reg),
            state.registers.gprs[reg.0 as usize].generation,
        )
    }

    pub fn optional_id_by_reg(&mut self, reg: Register, generation: u32) -> Option<VarId> {
        let reg = RegisterWithGeneration { reg, generation };
        self.reg_to_var.get(&reg).copied()
    }

    #[track_caller]
    pub fn id_by_reg(&mut self, reg: Register, generation: u32) -> VarId {
        match self.optional_id_by_reg(reg, generation) {
            Some(var_id) => var_id,
            None => panic!("no variable for {:?}_{:?}", reg, generation),
        }
    }

    pub fn mk_root_stack_mem_var(&mut self, offset: i16, vis: VariableVisibility) -> VarId {
        let key = self.list.push_and_get_key(Variable { vis });
        let addr = StackRelativeAddress { offset };
        assert!(self.mem_to_var.insert(addr, key).is_none());
        key
    }

    #[track_caller]
    pub fn mk_stack_mem_var(&mut self, offset: i16, origin: VarId) -> VarId {
        let key = self.list.push_and_get_key(Variable {
            vis: self.list[origin].vis,
        });
        let addr = StackRelativeAddress { offset };
        assert!(self.mem_to_var.insert(addr, key).is_none());
        key
    }

    #[track_caller]
    pub fn id_by_stack_mem(&mut self, offset: i16) -> VarId {
        let addr = StackRelativeAddress { offset };

        match self.mem_to_var.get(&addr) {
            Some(var_id) => *var_id,
            None => panic!("no variable for stack-relative addr {addr:?}"),
        }
    }
}

pub fn variable_map() -> Variables {
    todo!()
}
