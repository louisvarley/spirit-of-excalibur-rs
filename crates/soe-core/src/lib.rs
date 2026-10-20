//! Explicit 16-bit helpers used by the rest of the port.
//!
//! The original is 16-bit real-mode Turbo C. Rust's default overflow checks,
//! widening rules and signed comparisons are not a substitute for the original
//! machine semantics, so the port uses these helpers deliberately.

#![forbid(unsafe_code)]

use core::cmp::Ordering;

/// Wrapping 16-bit arithmetic.
pub mod arith {
    /// Wrapping unsigned add.
    #[must_use]
    pub const fn add_u16(a: u16, b: u16) -> u16 {
        a.wrapping_add(b)
    }

    /// Wrapping unsigned subtract.
    #[must_use]
    pub const fn sub_u16(a: u16, b: u16) -> u16 {
        a.wrapping_sub(b)
    }

    /// Wrapping unsigned multiply (low 16 bits).
    #[must_use]
    pub const fn mul_u16(a: u16, b: u16) -> u16 {
        a.wrapping_mul(b)
    }

    /// Logical left shift with a masked x86-style count.
    #[must_use]
    pub const fn shl_u16(a: u16, count: u16) -> u16 {
        a.wrapping_shl((count & 0x1f) as u32)
    }

    /// Logical right shift with a masked x86-style count.
    #[must_use]
    pub const fn shr_u16(a: u16, count: u16) -> u16 {
        a.wrapping_shr((count & 0x1f) as u32)
    }

    /// Arithmetic right shift on a signed 16-bit value.
    #[must_use]
    pub const fn sar_i16(a: i16, count: u16) -> i16 {
        a.wrapping_shr((count & 0x1f) as u32)
    }

    /// Two's-complement negation of an unsigned 16-bit value.
    #[must_use]
    pub const fn neg_u16(a: u16) -> u16 {
        a.wrapping_neg()
    }

    /// Wrapping increment.
    #[must_use]
    pub const fn inc_u16(a: u16) -> u16 {
        a.wrapping_add(1)
    }

    /// Wrapping decrement.
    #[must_use]
    pub const fn dec_u16(a: u16) -> u16 {
        a.wrapping_sub(1)
    }

    /// Wrapping signed add.
    #[must_use]
    pub const fn add_i16(a: i16, b: i16) -> i16 {
        a.wrapping_add(b)
    }

    /// Wrapping signed subtract.
    #[must_use]
    pub const fn sub_i16(a: i16, b: i16) -> i16 {
        a.wrapping_sub(b)
    }
}

/// Pack a DX:AX pair into a `u32`, with DX as the high word.
#[must_use]
pub const fn pack_dx_ax(dx: u16, ax: u16) -> u32 {
    (dx as u32) << 16 | ax as u32
}

/// Unpack a `u32` into `(DX, AX)`.
#[must_use]
pub const fn unpack_dx_ax(value: u32) -> (u16, u16) {
    ((value >> 16) as u16, value as u16)
}

/// Pack a `(high, low)` pair; the naming is useful at call sites.
#[must_use]
pub const fn pack_high_low(high: u16, low: u16) -> u32 {
    pack_dx_ax(high, low)
}

/// Unpack into `(high, low)`.
#[must_use]
pub const fn unpack_high_low(value: u32) -> (u16, u16) {
    unpack_dx_ax(value)
}

/// Compare two words with unsigned x86 semantics.
#[must_use]
pub const fn cmp_u16(a: u16, b: u16) -> Ordering {
    if a < b {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// Compare two words with signed x86 semantics.
#[must_use]
pub const fn cmp_i16(a: i16, b: i16) -> Ordering {
    if a < b {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// x86 `JBE` on unsigned words.
#[must_use]
pub const fn u16_below_or_equal(a: u16, b: u16) -> bool {
    a <= b
}

/// x86 `JA` on unsigned words.
#[must_use]
pub const fn u16_above(a: u16, b: u16) -> bool {
    a > b
}

/// x86 `JLE` on signed words.
#[must_use]
pub const fn i16_less_or_equal(a: i16, b: i16) -> bool {
    a <= b
}

/// x86 `JG` on signed words.
#[must_use]
pub const fn i16_greater(a: i16, b: i16) -> bool {
    a > b
}

/// A real-mode `segment:offset` far pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct FarPtr {
    pub segment: u16,
    pub offset: u16,
}

impl FarPtr {
    /// Construct a far pointer.
    #[must_use]
    pub const fn new(segment: u16, offset: u16) -> Self {
        Self { segment, offset }
    }

    /// The null pointer.
    #[must_use]
    pub const fn null() -> Self {
        Self::new(0, 0)
    }

    /// True when both halves are zero.
    #[must_use]
    pub const fn is_null(self) -> bool {
        self.segment == 0 && self.offset == 0
    }

    /// Linear address under real-mode `segment * 16 + offset`.
    #[must_use]
    pub const fn linear(self) -> u32 {
        (self.segment as u32) * 16 + self.offset as u32
    }

    /// Normalize a linear address to a canonical `segment:offset` far pointer.
    ///
    /// The offset is the low nibble, as in the original's normalized-pointer
    /// helpers (`1000:0313`, `1000:0396`, `1000:052a`).
    #[must_use]
    pub const fn from_linear(linear: u32) -> Self {
        Self::new(((linear >> 4) & 0xffff) as u16, (linear & 0xf) as u16)
    }

    /// Add an offset to the offset half with 16-bit wrap.
    #[must_use]
    pub const fn wrapping_add_offset(self, delta: u16) -> Self {
        Self::new(self.segment, self.offset.wrapping_add(delta))
    }

    /// Add a signed displacement to the offset half with 16-bit wrap.
    #[must_use]
    pub const fn wrapping_add_i16(self, delta: i16) -> Self {
        Self::new(self.segment, self.offset.wrapping_add(delta as u16))
    }

    /// Add a linear delta and normalize the result.
    #[must_use]
    pub const fn wrapping_add_linear(self, delta: i32) -> Self {
        let linear = self.linear().wrapping_add(delta as u32);
        Self::from_linear(linear)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_ops_match_16_bit_machine() {
        assert_eq!(arith::add_u16(0xffff, 1), 0);
        assert_eq!(arith::sub_u16(0, 1), 0xffff);
        assert_eq!(arith::mul_u16(0x100, 0x100), 0);
        assert_eq!(arith::neg_u16(1), 0xffff);
    }

    #[test]
    fn shifts_mask_the_count_like_x86() {
        assert_eq!(arith::shl_u16(1, 16), 1);
        assert_eq!(arith::shr_u16(0x8000, 1), 0x4000);
        assert_eq!(arith::sar_i16(-2, 1), -1);
        assert_eq!(arith::sar_i16(0x4000, 2), 0x1000);
    }

    #[test]
    fn dx_ax_round_trip() {
        assert_eq!(pack_dx_ax(0x1234, 0x5678), 0x1234_5678);
        assert_eq!(unpack_dx_ax(0x1234_5678), (0x1234, 0x5678));
        assert_eq!(pack_high_low(0xabcd, 0xef01), 0xabcd_ef01);
    }

    #[test]
    fn signed_and_unsigned_compares_differ() {
        let a = 0xffff_u16;
        let b = 1_u16;
        assert!(u16_above(a, b));
        assert!(!i16_greater(-1, 1));
        assert!(i16_less_or_equal(-1, 1));
        assert!(!u16_below_or_equal(a, b));
    }

    #[test]
    fn far_pointer_linear_and_normalize() {
        let p = FarPtr::new(0x2d65, 0x0adf);
        assert_eq!(p.linear(), 0x2e12f);
        let n = FarPtr::from_linear(0x2e12f);
        assert_eq!(n.linear(), 0x2e12f);
        assert_eq!(p.wrapping_add_offset(1), FarPtr::new(0x2d65, 0x0ae0));
        assert_eq!(FarPtr::new(0xffff, 0xffff).wrapping_add_offset(1).offset, 0);
    }
}
