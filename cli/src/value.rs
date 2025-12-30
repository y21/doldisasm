use std::fmt::Debug;

use arrayvec::ArrayVec;
use bumpalo::Bump;
use either::Either;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Parameter(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VInt {
    pub val: u32,
    pub ty: IntType,
}

impl VInt {
    fn new(val: u32, ty: IntType) -> Self {
        // truncate value
        let val = match ty {
            IntType::I8 => val as i8 as u32,
            IntType::I16 => val as i16 as u32,
            IntType::I32 => val as i32 as u32,
            IntType::U8 => val as u8 as u32,
            IntType::U16 => val as u16 as u32,
            IntType::U32 | IntType::Ptr | IntType::F32 => val,
        };
        Self { val, ty }
    }

    #[rustfmt::skip]
    pub fn add(self, other: Self) -> Option<Self> {
        use IntType::*;

        macro_rules! signed {
            () => { I8 | I16 | I32 };
        }
        macro_rules! unsigned {
            () => { U8 | U16 | U32 | Ptr };
        }
        macro_rules! int {
            () => { signed!() | unsigned!() };
        }
        macro_rules! float {
            () => { F32 };
        }

        let (val, ty) = match (self.ty, other.ty) {
            (float!(), float!()) => (f32::to_bits(f32::from_bits(self.val) + f32::from_bits(other.val)), self.ty),
            (float!(), _) | (_, float!()) => return None,
            (int!(), int!()) => (self.val.wrapping_add(other.val), self.ty),
        };

        Some(Self::new(val, ty))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntType {
    I8,
    I16,
    I32,
    U8,
    U16,
    U32,
    F32,
    Ptr,
    // Infer,
}

impl IntType {
    fn is_uint(&self) -> bool {
        matches!(
            self,
            IntType::U8 | IntType::U16 | IntType::U32 | IntType::Ptr
        )
    }
    fn is_sint(&self) -> bool {
        matches!(self, IntType::I8 | IntType::I16 | IntType::I32)
    }
    fn is_float(&self) -> bool {
        matches!(self, IntType::F32)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ValueInner<'bump> {
    #[default]
    Uninitialized,
    CallerStack,
    Param(Parameter),
    Int(VInt),
    Add(&'bump Value<'bump>, &'bump Value<'bump>),
    BitOr(&'bump Value<'bump>, &'bump Value<'bump>),
    OneIfNegative(&'bump Value<'bump>),
    OneIfPositive(&'bump Value<'bump>),
    OneIfZero(&'bump Value<'bump>),
    /// The result of a bl
    CallResult(u32),
    ReturnAddress,
    Any,
}

impl Debug for ValueInner<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            ValueInner::Uninitialized => write!(f, "<uninit>"),
            ValueInner::CallerStack => write!(f, "<caller_stack>"),
            ValueInner::Param(p) => write!(f, "<param {:?}>", p),
            ValueInner::Int(VInt { val, ty }) => match ty {
                IntType::I8 => write!(f, "{}", val as i8),
                IntType::I16 => write!(f, "{}", val as i16),
                IntType::I32 => write!(f, "{}", val as i32),
                IntType::U8 => write!(f, "{}", val as u8),
                IntType::U16 => write!(f, "{}", val as u16),
                IntType::U32 => write!(f, "{}", val),
                IntType::F32 => write!(f, "{}", f32::from_bits(val)),
                IntType::Ptr => write!(f, "0x{:08x}", val),
                // IntType::Infer => write!(f, "{}", *imm),
            },
            ValueInner::Add(a, b) => write!(f, "({:?} + {:?})", a, b),
            ValueInner::BitOr(a, b) => write!(f, "({:?} | {:?})", a, b),
            ValueInner::OneIfNegative(v) => write!(f, "one_if_negative({:?})", v),
            ValueInner::OneIfPositive(v) => write!(f, "one_if_positive({:?})", v),
            ValueInner::OneIfZero(v) => write!(f, "one_if_zero({:?})", v),
            ValueInner::CallResult(addr) => write!(f, "<retval of call to 0x{:08x}>", addr),
            ValueInner::ReturnAddress => write!(f, "<return_address>"),
            ValueInner::Any => write!(f, "<any>"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Value<'bump>(ValueInner<'bump>);

impl Debug for Value<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

impl<'bump> Value<'bump> {
    pub const ZERO_U32: Self = Self::u32(0);
    pub const UNINIT: Self = Self(ValueInner::Uninitialized);
    pub const ANY: Self = Self(ValueInner::Any);
    pub const CALLER_STACK: Self = Self(ValueInner::CallerStack);
    pub const RETURN_ADDRESS: Self = Self(ValueInner::ReturnAddress);

    pub fn join(self, other: Self) -> Self {
        if self == other {
            self
        } else if self == Value::UNINIT || other == Value::UNINIT {
            Value::UNINIT
        } else {
            Value::ANY
        }
    }

    pub fn is_initialized(self) -> bool {
        !matches!(self.0, ValueInner::Uninitialized)
    }

    pub const fn u32(imm: u32) -> Self {
        Self::int(imm, IntType::U32)
    }

    pub fn i16(imm: i16) -> Self {
        Self::int(imm as u32, IntType::I16)
    }

    pub const fn int(imm: u32, int_type: IntType) -> Self {
        Self(ValueInner::Int(VInt {
            val: imm,
            ty: int_type,
        }))
    }

    pub fn add(self, other: Self, bump: &'bump Bump) -> Self {
        // Canonicalization

        let canon_iter = [self, other].into_iter().flat_map(|val| match val.0 {
            ValueInner::Add(left, right) => Either::Left([*left, *right].into_iter()),
            _ => Either::Right([val].into_iter()),
        });

        let mut sum: Option<VInt> = None;
        let mut terms = ArrayVec::<Value<'bump>, 4>::new();

        for value in canon_iter {
            if let ValueInner::Int(imm) = value.0 {
                if let Some(existing_sum) = &mut sum {
                    if let Some(new_sum) = existing_sum.add(imm) {
                        *existing_sum = new_sum;
                    } else {
                        // cannot combine
                        terms.push(value);
                    }
                } else {
                    sum = Some(imm);
                }
            } else {
                terms.push(value);
            }
        }

        let wrap_add = |l, r| Self(ValueInner::Add(bump.alloc(l), bump.alloc(r)));

        match *terms {
            [] => Self(ValueInner::Int(sum.unwrap())),
            [single] => {
                let sum = sum.unwrap();
                if sum.val == 0 {
                    single
                } else {
                    wrap_add(single, Self(ValueInner::Int(sum)))
                }
            }
            [a, b] => {
                if let Some(sum) = sum.filter(|v| v.val != 0) {
                    wrap_add(wrap_add(a, b), Self(ValueInner::Int(sum)))
                } else {
                    wrap_add(a, b)
                }
            }
            [a, b, c] => {
                if let Some(sum) = sum.filter(|v| v.val != 0) {
                    wrap_add(wrap_add(a, b), wrap_add(c, Self(ValueInner::Int(sum))))
                } else {
                    wrap_add(wrap_add(a, b), c)
                }
            }
            [a, b, c, d] => {
                if let Some(sum) = sum.filter(|v| v.val != 0) {
                    wrap_add(
                        wrap_add(a, b),
                        wrap_add(c, wrap_add(d, Self(ValueInner::Int(sum)))),
                    )
                } else {
                    wrap_add(wrap_add(a, b), wrap_add(c, d))
                }
            }
            _ => unreachable!(),
        }
    }

    pub fn call_result(addr: u32) -> Self {
        Self(ValueInner::CallResult(addr))
    }

    pub fn bit_or(self, other: Self, bump: &'bump Bump) -> Self {
        if let ValueInner::Int(left) = self.0
            && let ValueInner::Int(right) = other.0
        {
            assert_eq!(left.ty, right.ty); // for now
            return Self::int(left.val | right.val, left.ty);
        }

        if self > other {
            // canonicalize order
            return other.bit_or(self, bump);
        }

        Self(ValueInner::BitOr(bump.alloc(self), bump.alloc(other)))
    }

    pub fn one_if_negative(self, bump: &'bump Bump) -> Self {
        if let ValueInner::Int(imm) = self.0 {
            Self::u32(if (imm.val as i32) < 0 { 1 } else { 0 })
        } else {
            Self(ValueInner::OneIfNegative(bump.alloc(self)))
        }
    }

    pub fn one_if_positive(self, bump: &'bump Bump) -> Self {
        if let ValueInner::Int(imm) = self.0 {
            Self::u32(if (imm.val as i32) > 0 { 1 } else { 0 })
        } else {
            Self(ValueInner::OneIfPositive(bump.alloc(self)))
        }
    }

    pub fn one_if_zero(self, bump: &'bump Bump) -> Self {
        if let ValueInner::Int(imm) = self.0 {
            Self::u32(if imm.val == 0 { 1 } else { 0 })
        } else {
            Self(ValueInner::OneIfZero(bump.alloc(self)))
        }
    }

    pub fn parameter(param: Parameter) -> Self {
        Self(ValueInner::Param(param))
    }

    pub fn inner(self) -> ValueInner<'bump> {
        self.0
    }
}
