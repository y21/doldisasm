use crate::dataflow::core::Join;

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct RegisterState<S> {
    pub gprs: [S; 32],
    pub sprs: SprState<S>,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct SprState<S> {
    pub lr: S,
    pub ctr: S,
    pub xer: XerState<S>,
    pub msr: S,
    pub cr: [CrFieldState<S>; 8],
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

impl<S: Join<T>, T> Join<RegisterState<T>> for RegisterState<S> {
    fn join(&self, other: &Self, arg: &mut RegisterState<T>) -> Self {
        Self {
            gprs: Join::join(&self.gprs, &other.gprs, &mut arg.gprs),
            sprs: Join::join(&self.sprs, &other.sprs, &mut arg.sprs),
        }
    }
}

impl<S: Join<T>, T> Join<SprState<T>> for SprState<S> {
    fn join(&self, other: &Self, arg: &mut SprState<T>) -> Self {
        Self {
            lr: Join::join(&self.lr, &other.lr, &mut arg.lr),
            ctr: Join::join(&self.ctr, &other.ctr, &mut arg.lr),
            xer: Join::join(&self.xer, &other.xer, &mut arg.xer),
            msr: Join::join(&self.msr, &other.msr, &mut arg.msr),
            cr: Join::join(&self.cr, &other.cr, &mut arg.cr),
        }
    }
}

impl<S: Join<T>, T> Join<XerState<T>> for XerState<S> {
    fn join(&self, other: &Self, arg: &mut XerState<T>) -> Self {
        Self {
            so: Join::join(&self.so, &other.so, &mut arg.so),
            ov: Join::join(&self.ov, &other.ov, &mut arg.ov),
            ca: Join::join(&self.ca, &other.ca, &mut arg.ca),
        }
    }
}

impl<S: Join<T>, T> Join<CrFieldState<T>> for CrFieldState<S> {
    fn join(&self, other: &Self, arg: &mut CrFieldState<T>) -> Self {
        Self {
            lt: Join::join(&self.lt, &other.lt, &mut arg.lt),
            gt: Join::join(&self.gt, &other.gt, &mut arg.gt),
            eq: Join::join(&self.eq, &other.eq, &mut arg.eq),
            so: Join::join(&self.so, &other.so, &mut arg.so),
        }
    }
}
