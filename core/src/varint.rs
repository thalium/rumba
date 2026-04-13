use std::{
    fmt::Display,
    ops::{Add, BitAnd, BitOr, BitXor, Deref, Mul, Neg, Not, Sub},
};

pub const fn make_mask(n: u8) -> u64 {
    if n >= 64 { u64::MAX } else { (1u64 << n) - 1 }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarInt(u64);

impl VarInt {
    pub const MAX: Self = Self(u64::MAX);

    pub const ZERO: Self = Self(0);

    pub const ONE: Self = Self(1);

    pub const fn get(self, mask: u64) -> u64 {
        self.0 & mask
    }

    pub const fn mask(self, mask: u64) -> Self {
        Self(self.get(mask))
    }

    pub const fn get_signed(self, n: u8, mask: u64) -> i64 {
        let value = self.get(mask);
        let shift = 64 - n;
        ((value << shift) as i64) >> shift
    }

    /// Displays a varint properly (with the best sign)
    pub fn repr(self, n: u8, mask: u64, hex: bool, latex: bool) -> String {
        let val = self.get(mask);
        let sign_bit = 1u64 << (n - 1);

        if val & sign_bit != 0 {
            let signed = self.get_signed(n, mask);

            match (hex, latex) {
                (true, true) => format!("(-\\mathrm{{{:#x}}})", -signed),
                (true, false) => format!("(-{:#x})", -signed),
                _ => format!("(-{})", -signed),
            }
        } else {
            match (hex, latex) {
                (true, true) => format!("\\mathrm{{{:#x}}}", val),
                (true, false) => format!("{:#x}", val),
                _ => format!("{}", val),
            }
        }
    }
}

impl From<u64> for VarInt {
    fn from(value: u64) -> Self {
        VarInt(value)
    }
}

impl Deref for VarInt {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for VarInt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.repr(64, u64::MAX, false, false))
    }
}

impl Not for VarInt {
    type Output = VarInt;

    fn not(self) -> Self::Output {
        (!self.0).into()
    }
}

impl Neg for VarInt {
    type Output = VarInt;

    fn neg(self) -> Self::Output {
        self.0.wrapping_neg().into()
    }
}

impl BitXor for VarInt {
    type Output = VarInt;

    fn bitxor(self, rhs: Self) -> Self::Output {
        (self.0 ^ rhs.0).into()
    }
}

impl BitOr for VarInt {
    type Output = VarInt;

    fn bitor(self, rhs: Self) -> Self::Output {
        (self.0 | rhs.0).into()
    }
}

impl BitAnd for VarInt {
    type Output = VarInt;

    fn bitand(self, rhs: Self) -> Self::Output {
        (self.0 & rhs.0).into()
    }
}

impl Add for VarInt {
    type Output = VarInt;

    fn add(self, rhs: Self) -> Self::Output {
        self.0.wrapping_add(rhs.0).into()
    }
}

impl Sub for VarInt {
    type Output = VarInt;

    fn sub(self, rhs: Self) -> Self::Output {
        self.0.wrapping_sub(rhs.0).into()
    }
}

impl Mul for VarInt {
    type Output = VarInt;

    fn mul(self, rhs: Self) -> Self::Output {
        self.0.wrapping_mul(rhs.0).into()
    }
}
