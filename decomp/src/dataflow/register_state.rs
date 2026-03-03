#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct RegisterState<S> {
    pub gprs: [S; 32],
    pub sprs: SprState<S>,
}

impl<S> RegisterState<S> {
    pub fn states_iter(&mut self) -> impl Iterator<Item = &mut S> {
        let SprState {
            lr,
            ctr,
            xer: XerState { so, ov, ca },
            msr,
            cr,
        } = &mut self.sprs;

        self.gprs
            .iter_mut()
            .chain([lr, ctr, so, ov, ca, msr])
            .chain(
                cr.iter_mut()
                    .flat_map(|CrFieldState { lt, gt, eq, so }| [lt, gt, eq, so]),
            )
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct SprState<S> {
    pub lr: S,
    pub ctr: S,
    pub xer: XerState<S>,
    pub msr: S,
    pub cr: [CrFieldState<S>; 8],
}

impl<S> SprState<S> {
    pub fn cr(&self, crf: ppc32::instruction::Crf, crb: ppc32::instruction::Crb) -> &S {
        let field = &self.cr[crf.0 as usize];
        match crb {
            ppc32::instruction::Crb::Negative => &field.lt,
            ppc32::instruction::Crb::Positive => &field.gt,
            ppc32::instruction::Crb::Zero => &field.eq,
            ppc32::instruction::Crb::Overflow => &field.so,
        }
    }

    pub fn cr_mut(&mut self, crf: ppc32::instruction::Crf, crb: ppc32::instruction::Crb) -> &mut S {
        let field = &mut self.cr[crf.0 as usize];
        match crb {
            ppc32::instruction::Crb::Negative => &mut field.lt,
            ppc32::instruction::Crb::Positive => &mut field.gt,
            ppc32::instruction::Crb::Zero => &mut field.eq,
            ppc32::instruction::Crb::Overflow => &mut field.so,
        }
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct CrFieldState<S> {
    pub lt: S,
    pub gt: S,
    pub eq: S,
    pub so: S,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct XerState<S> {
    pub so: S,
    pub ov: S,
    pub ca: S,
}
