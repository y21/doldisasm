use ppc32::instruction::{Crb, Crf, Gpr, Register, Spr, XerRegister};

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
    pub fn by_register(&mut self, reg: Register) -> &mut S {
        match reg {
            Register::Gpr(gpr) => &mut self.gprs[gpr.0 as usize],
            Register::Cr(crf, crb) => self.sprs.cr_mut(crf, crb),
            Register::Spr(Spr::Ctr) => &mut self.sprs.ctr,
            Register::Spr(Spr::Lr) => &mut self.sprs.lr,
            Register::Spr(Spr::Msr) => &mut self.sprs.msr,
            Register::Spr(Spr::Xer(XerRegister::So)) => &mut self.sprs.xer.so,
            Register::Spr(Spr::Xer(XerRegister::Ov)) => &mut self.sprs.xer.ov,
            Register::Spr(Spr::Xer(XerRegister::Ca)) => &mut self.sprs.xer.ca,
            Register::Spr(Spr::Pc | Spr::Other(_)) => todo!(),
        }
    }

    pub fn register_iter(&self) -> impl Iterator<Item = (Register, &S)> {
        let Self {
            gprs,
            sprs:
                SprState {
                    lr,
                    ctr,
                    xer: _, // TODO: handle xer
                    msr,
                    cr,
                },
        } = self;

        let gprs = gprs
            .iter()
            .enumerate()
            .map(|(gpr, state)| (Register::Gpr(Gpr(gpr as u8)), state));

        let sprs = [(Spr::Lr, lr), (Spr::Ctr, ctr), (Spr::Msr, msr)]
            .into_iter()
            .map(|(spr, state)| (Register::Spr(spr), state));

        let crs = cr.iter().enumerate().flat_map(|(crf, cr)| {
            let crf = Crf(crf as u8);
            [
                (Register::Cr(crf, Crb::Negative), &cr.lt),
                (Register::Cr(crf, Crb::Positive), &cr.gt),
                (Register::Cr(crf, Crb::Zero), &cr.eq),
                (Register::Cr(crf, Crb::Overflow), &cr.so),
            ]
        });

        gprs.chain(sprs).chain(crs)
    }

    pub fn register_iter_mut(&mut self) -> impl Iterator<Item = (Register, &mut S)> {
        // NOTE: this is identical to `register_iter`
        // TODO: figure out if we can deduplicate
        let Self {
            gprs,
            sprs:
                SprState {
                    lr,
                    ctr,
                    xer: _, // TODO: handle xer
                    msr,
                    cr,
                },
        } = self;

        let gprs = gprs
            .iter_mut()
            .enumerate()
            .map(|(gpr, state)| (Register::Gpr(Gpr(gpr as u8)), state));

        let sprs = [(Spr::Lr, lr), (Spr::Ctr, ctr), (Spr::Msr, msr)]
            .into_iter()
            .map(|(spr, state)| (Register::Spr(spr), state));

        let crs = cr.iter_mut().enumerate().flat_map(|(crf, cr)| {
            let crf = Crf(crf as u8);
            [
                (Register::Cr(crf, Crb::Negative), &mut cr.lt),
                (Register::Cr(crf, Crb::Positive), &mut cr.gt),
                (Register::Cr(crf, Crb::Zero), &mut cr.eq),
                (Register::Cr(crf, Crb::Overflow), &mut cr.so),
            ]
        });

        gprs.chain(sprs).chain(crs)
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
