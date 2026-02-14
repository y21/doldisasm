use ppc32::instruction::Spr;

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
