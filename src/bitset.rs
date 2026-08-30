//! A basic bitset impl

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct BitSet(u32);

pub struct BitSetIter(u32);

impl BitSet {
    pub fn get(self, ix: usize) -> bool {
        (self.0 >> ix) & 1 != 0
    }

    pub fn insert(&mut self, ix: usize) {
        self.0 |= 1 << ix;
    }

    #[expect(unused)]
    pub fn remove(&mut self, ix: usize) {
        self.0 &= !(1 << ix);
    }

    #[expect(unused)]
    pub fn toggle(&mut self, ix: usize) {
        self.0 ^= 1 << ix;
    }

    #[expect(unused)]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl core::ops::BitAnd for BitSet {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl core::ops::BitAndAssign for BitSet {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl core::ops::BitOr for BitSet {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for BitSet {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl core::ops::Not for BitSet {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(!self.0)
    }
}

impl IntoIterator for BitSet {
    type Item = usize;

    type IntoIter = BitSetIter;

    fn into_iter(self) -> Self::IntoIter {
        BitSetIter(self.0)
    }
}

impl Iterator for BitSetIter {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0 == 0 {
            None
        } else {
            let result = self.0.trailing_zeros();
            self.0 &= self.0 - 1;
            Some(result as usize)
        }
    }
}
