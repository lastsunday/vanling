use rand::Rng;
use rand::rngs::OsRng;
use std::sync::{Mutex, OnceLock};

const POOL_SIZE: usize = 1_000_000;
const WORD_COUNT: usize = POOL_SIZE / 64;

static POOL: OnceLock<Mutex<ActivationPool>> = OnceLock::new();

pub struct ActivationPool {
    bits: Box<[u64; WORD_COUNT]>,
    free: u32,
}

impl ActivationPool {
    pub fn init(used_codes: &[u32]) {
        let mut pool = Self {
            bits: Box::new([0u64; WORD_COUNT]),
            free: POOL_SIZE as u32,
        };
        for &code in used_codes {
            if code < POOL_SIZE as u32 {
                let (idx, bit) = (code as usize / 64, 1u64 << (code as usize % 64));
                if pool.bits[idx] & bit == 0 {
                    pool.bits[idx] |= bit;
                    pool.free -= 1;
                }
            }
        }
        POOL.set(Mutex::new(pool)).ok();
    }

    pub fn global() -> &'static Mutex<Self> {
        POOL.get().expect("ActivationPool not initialized")
    }

    pub fn draw(&mut self) -> u32 {
        let n = OsRng.gen_range(0..self.free);
        let code = self.select_zero(n);
        let (idx, bit) = (code as usize / 64, 1u64 << (code as usize % 64));
        self.bits[idx] |= bit;
        self.free -= 1;
        code
    }

    pub fn discard(&mut self, code: u32) {
        if code >= POOL_SIZE as u32 {
            return;
        }
        let (idx, bit) = (code as usize / 64, 1u64 << (code as usize % 64));
        if self.bits[idx] & bit != 0 {
            self.bits[idx] &= !bit;
            self.free += 1;
        }
    }

    pub fn is_used(&self, code: u32) -> bool {
        if code >= POOL_SIZE as u32 {
            return true;
        }
        let (idx, bit) = (code as usize / 64, 1u64 << (code as usize % 64));
        self.bits[idx] & bit != 0
    }

    fn select_zero(&self, n: u32) -> u32 {
        let mut remaining = n;
        for (i, &word) in self.bits.iter().enumerate() {
            let zeros = 64 - word.count_ones();
            if remaining < zeros {
                let pos = nth_zero_bit(word, remaining);
                return (i * 64 + pos as usize) as u32;
            }
            remaining -= zeros;
        }
        unreachable!("select_zero called with n >= free count")
    }
}

fn nth_zero_bit(word: u64, n: u32) -> u32 {
    let mut lo = 0u64;
    let mut hi = 64u64;
    while lo < hi {
        let mid = (lo + hi) / 2;
        let mask = (1u64 << mid) - 1;
        let zeros_in_prefix = mid - (word & mask).count_ones() as u64;
        if zeros_in_prefix > n as u64 {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo as u32 - 1
}
