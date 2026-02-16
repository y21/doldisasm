pub struct Ty {
    pub kind: TyKind,
}

pub enum TyKind {
    I32,
    U32,
    Void,
}
