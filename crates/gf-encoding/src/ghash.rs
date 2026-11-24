//! Port of `encoding/ghash`: classic 32-bit and 64-bit hash functions.
//!
//! Go's unsigned arithmetic wraps on overflow; every operation that can
//! overflow here uses the explicit wrapping form.

/// AP hash, 32 bits.
#[inline]
pub fn ap(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0;
    for (i, &byte) in bytes.iter().enumerate() {
        if (i & 1) == 0 {
            hash ^= (hash << 7) ^ u32::from(byte) ^ (hash >> 3);
        } else {
            hash ^= ((hash << 11) ^ u32::from(byte) ^ (hash >> 5)).wrapping_neg();
        }
    }
    hash
}

/// AP hash, 64 bits.
#[inline]
pub fn ap64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0;
    for (i, &byte) in bytes.iter().enumerate() {
        if (i & 1) == 0 {
            hash ^= (hash << 7) ^ u64::from(byte) ^ (hash >> 3);
        } else {
            hash ^= ((hash << 11) ^ u64::from(byte) ^ (hash >> 5)).wrapping_neg();
        }
    }
    hash
}

/// BKDR hash, 32 bits.
#[inline]
pub fn bkdr(bytes: &[u8]) -> u32 {
    const SEED: u32 = 131;
    let mut hash: u32 = 0;
    for &byte in bytes {
        hash = hash.wrapping_mul(SEED).wrapping_add(u32::from(byte));
    }
    hash
}

/// BKDR hash, 64 bits.
#[inline]
pub fn bkdr64(bytes: &[u8]) -> u64 {
    const SEED: u64 = 131;
    let mut hash: u64 = 0;
    for &byte in bytes {
        hash = hash.wrapping_mul(SEED).wrapping_add(u64::from(byte));
    }
    hash
}

/// DJB hash, 32 bits.
#[inline]
pub fn djb(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 5381;
    for &byte in bytes {
        hash = hash.wrapping_add(hash << 5).wrapping_add(u32::from(byte));
    }
    hash
}

/// DJB hash, 64 bits.
#[inline]
pub fn djb64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 5381;
    for &byte in bytes {
        hash = hash.wrapping_add(hash << 5).wrapping_add(u64::from(byte));
    }
    hash
}

/// ELF hash, 32 bits.
#[inline]
pub fn elf(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0;
    for &byte in bytes {
        hash = (hash << 4).wrapping_add(u32::from(byte));
        let x = hash & 0xF000_0000;
        if x != 0 {
            hash ^= x >> 24;
            hash &= x.wrapping_neg();
        }
    }
    hash
}

/// ELF hash, 64 bits.
#[inline]
pub fn elf64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0;
    for &byte in bytes {
        hash = (hash << 8).wrapping_add(u64::from(byte));
        let x = hash & 0xFF00_0000_0000_0000;
        if x != 0 {
            hash ^= x >> 48;
            hash &= x.wrapping_neg();
        }
    }
    hash
}

/// JS hash, 32 bits.
#[inline]
pub fn js(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 1_315_423_911;
    for &byte in bytes {
        hash ^= (hash << 5)
            .wrapping_add(u32::from(byte))
            .wrapping_add(hash >> 2);
    }
    hash
}

/// JS hash, 64 bits.
#[inline]
pub fn js64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 1_315_423_911;
    for &byte in bytes {
        hash ^= (hash << 5)
            .wrapping_add(u64::from(byte))
            .wrapping_add(hash >> 2);
    }
    hash
}

/// PJW hash, 32 bits.
#[inline]
pub fn pjw(bytes: &[u8]) -> u32 {
    const BITS: u32 = 32;
    const THREE_QUARTERS: u32 = BITS * 3 / 4;
    const ONE_EIGHTH: u32 = BITS / 8;
    const HIGH_BITS: u32 = 0xFFFF_FFFFu32 << (BITS - ONE_EIGHTH);
    let mut hash: u32 = 0;
    for &byte in bytes {
        hash = (hash << ONE_EIGHTH).wrapping_add(u32::from(byte));
        let test = hash & HIGH_BITS;
        if test != 0 {
            hash = (hash ^ (test >> THREE_QUARTERS)) & HIGH_BITS.wrapping_neg();
        }
    }
    hash
}

/// PJW hash, 64 bits.
#[inline]
pub fn pjw64(bytes: &[u8]) -> u64 {
    const BITS: u64 = 64;
    const THREE_QUARTERS: u64 = BITS * 3 / 4;
    const ONE_EIGHTH: u64 = BITS / 8;
    const HIGH_BITS: u64 = 0xFFFF_FFFF_FFFF_FFFFu64 << (BITS - ONE_EIGHTH);
    let mut hash: u64 = 0;
    for &byte in bytes {
        hash = (hash << ONE_EIGHTH).wrapping_add(u64::from(byte));
        let test = hash & HIGH_BITS;
        if test != 0 {
            hash = (hash ^ (test >> THREE_QUARTERS)) & HIGH_BITS.wrapping_neg();
        }
    }
    hash
}

/// RS hash, 32 bits.
#[inline]
pub fn rs(bytes: &[u8]) -> u32 {
    const B: u32 = 378_551;
    const A: u32 = 63_689;
    let mut a: u32 = A;
    let mut hash: u32 = 0;
    for &byte in bytes {
        hash = hash.wrapping_mul(a).wrapping_add(u32::from(byte));
        a = a.wrapping_mul(B);
    }
    hash
}

/// RS hash, 64 bits.
#[inline]
pub fn rs64(bytes: &[u8]) -> u64 {
    const B: u64 = 378_551;
    const A: u64 = 63_689;
    let mut a: u64 = A;
    let mut hash: u64 = 0;
    for &byte in bytes {
        hash = hash.wrapping_mul(a).wrapping_add(u64::from(byte));
        a = a.wrapping_mul(B);
    }
    hash
}

/// SDBM hash, 32 bits.
#[inline]
pub fn sdbm(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0;
    for &byte in bytes {
        hash = u32::from(byte)
            .wrapping_add(hash << 6)
            .wrapping_add(hash << 16)
            .wrapping_sub(hash);
    }
    hash
}

/// SDBM hash, 64 bits.
#[inline]
pub fn sdbm64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0;
    for &byte in bytes {
        hash = u64::from(byte)
            .wrapping_add(hash << 6)
            .wrapping_add(hash << 16)
            .wrapping_sub(hash);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIC: &[u8] = b"This is the test string for hash.";

    #[test]
    fn golden_values_match_the_go_tests() {
        assert_eq!(bkdr(BASIC), 200_645_773);
        assert_eq!(bkdr64(BASIC), 4_214_762_819_217_104_013);
        assert_eq!(sdbm(BASIC), 1_069_170_245);
        assert_eq!(sdbm64(BASIC), 9_881_052_176_572_890_693);
        assert_eq!(rs(BASIC), 1_944_033_799);
        assert_eq!(rs64(BASIC), 13_439_708_950_444_349_959);
        assert_eq!(js(BASIC), 498_688_898);
        assert_eq!(js64(BASIC), 13_410_163_655_098_759_877);
        assert_eq!(pjw(BASIC), 7_244_206);
        assert_eq!(pjw64(BASIC), 46);
        assert_eq!(elf(BASIC), 7_244_206);
        assert_eq!(elf64(BASIC), 46);
        assert_eq!(djb(BASIC), 959_862_602);
        assert_eq!(djb64(BASIC), 2_519_720_351_310_960_458);
        assert_eq!(ap(BASIC), 3_998_202_516);
        assert_eq!(ap64(BASIC), 2_531_023_058_543_352_243);
    }

    #[test]
    fn empty_input_returns_the_initial_value() {
        assert_eq!(ap(b""), 0);
        assert_eq!(ap64(b""), 0);
        assert_eq!(bkdr(b""), 0);
        assert_eq!(bkdr64(b""), 0);
        assert_eq!(djb(b""), 5381);
        assert_eq!(djb64(b""), 5381);
        assert_eq!(elf(b""), 0);
        assert_eq!(elf64(b""), 0);
        assert_eq!(js(b""), 1_315_423_911);
        assert_eq!(js64(b""), 1_315_423_911);
        assert_eq!(pjw(b""), 0);
        assert_eq!(pjw64(b""), 0);
        assert_eq!(rs(b""), 0);
        assert_eq!(rs64(b""), 0);
        assert_eq!(sdbm(b""), 0);
        assert_eq!(sdbm64(b""), 0);
    }

    #[test]
    fn single_byte_follows_the_recurrence() {
        assert_eq!(bkdr(&[0x01]), 1);
        assert_eq!(bkdr(&[0xFF]), 255);
        assert_eq!(ap(&[0x2A]), 0x2A);
        assert_eq!(elf(&[0x2A]), 0x2A);
        assert_eq!(sdbm(&[0x2A]), 0x2A);
        assert_eq!(rs(&[0x2A]), 0x2A);
        assert_eq!(pjw(&[0x2A]), 0x2A);
    }
}
