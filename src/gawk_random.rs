//! gawk's random number generator, so `rand()` after `srand(n)` produces the
//! sequence gawk produces.
//!
//! A port of the BSD `random(3)` that gawk ships (gawk `support/random.c`):
//! the additive feedback generator `x**63 + x + 1` (`TYPE_4`, which gawk's
//! 256-byte state array selects), seeded through `good_rand` (Park–Miller),
//! warmed up by `10 * deg` draws, and wrapped in gawk's 512-entry Bays–Durham
//! shuffle. `rand()` itself is gawk's `do_rand` (builtin.c): two draws combined
//! into one double in `[0, 1)`.

/// `DEG_4`: the degree of the `TYPE_4` polynomial, and the state length.
const DEG: usize = 63;
/// `SEP_4`: the separation between the front and rear taps.
const SEP: usize = 1;
/// `SHUFFLE_MAX`: the shuffle buffer's size (a power of two).
const SHUFFLE_MAX: usize = 512;

/// The generator's whole state.
#[derive(Debug, Clone)]
pub struct GawkRandom {
    state: [u32; DEG],
    /// Front tap (`fptr`) and rear tap (`rptr`), as indices into `state`.
    front: usize,
    rear: usize,
    shuffle: [u32; SHUFFLE_MAX],
    /// `shuffle_init`: refill the shuffle buffer before the next draw.
    shuffle_pending: bool,
    /// The shuffle's carried index value (`s`).
    carry: u32,
}

impl Default for GawkRandom {
    /// gawk's state before any `srand`: `initstate(1, …)`.
    fn default() -> Self {
        let mut r = GawkRandom {
            state: [0; DEG],
            front: SEP,
            rear: 0,
            shuffle: [0; SHUFFLE_MAX],
            shuffle_pending: true,
            carry: 0,
        };
        r.seed(1);
        r
    }
}

/// `good_rand`: one Park–Miller step, computed without overflow (Schrage).
fn good_rand(x: i32) -> u32 {
    let x = if x == 0 { 123_459_876 } else { x };
    let (hi, lo) = (x / 127_773, x % 127_773);
    let mut x = 16_807 * lo - 2_836 * hi;
    if x < 0 {
        x += 0x7fff_ffff;
    }
    x as u32
}

impl GawkRandom {
    /// `srandom(seed)`.
    pub fn seed(&mut self, seed: u32) {
        self.shuffle_pending = true;
        self.state[0] = seed;
        for i in 1..DEG {
            self.state[i] = good_rand(self.state[i - 1] as i32);
        }
        self.front = SEP;
        self.rear = 0;
        for _ in 0..10 * DEG {
            self.next_raw();
        }
    }

    /// `random_old()`: one step of the additive feedback generator.
    fn step(&mut self) -> u32 {
        let (f, r) = (self.front, self.rear);
        self.state[f] = self.state[f].wrapping_add(self.state[r]);
        let out = (self.state[f] >> 1) & 0x7fff_ffff;
        self.front += 1;
        if self.front >= DEG {
            self.front = 0;
            self.rear += 1;
        } else {
            self.rear += 1;
            if self.rear >= DEG {
                self.rear = 0;
            }
        }
        out
    }

    /// `random()`: the generator through the shuffle buffer, in `[0, 2^31)`.
    pub fn next_raw(&mut self) -> u32 {
        if self.shuffle_pending {
            for k in 0..SHUFFLE_MAX {
                self.shuffle[k] = self.step();
            }
            self.carry = self.step();
            self.shuffle_pending = false;
        }
        let r = self.step();
        let k = self.carry as usize & (SHUFFLE_MAX - 1);
        self.carry = self.shuffle[k];
        self.shuffle[k] = r;
        self.carry
    }

    /// gawk `do_rand`: two draws combined into a double in `[0, 1)`.
    pub fn next_f64(&mut self) -> f64 {
        const DIVISOR: f64 = 2_147_483_648.0; // GAWK_RANDOM_MAX + 1
        loop {
            let d1 = f64::from(self.next_raw());
            let d2 = f64::from(self.next_raw());
            let v = 0.5 + ((d1 / DIVISOR + d2) / DIVISOR) - 0.5;
            if v != 1.0 {
                return v;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GawkRandom;

    /// gawk 5.4.1: `BEGIN { srand(1); print rand(), rand() }` prints
    /// `0.924046 0.593909`, and without `srand` the first value is the same.
    #[test]
    fn matches_gawk_sequence() {
        let mut r = GawkRandom::default();
        r.seed(1);
        let a = r.next_f64();
        let b = r.next_f64();
        assert_eq!(format!("{a:.6} {b:.6}"), "0.924046 0.593909");
        let mut fresh = GawkRandom::default();
        assert_eq!(format!("{:.6}", fresh.next_f64()), "0.924046");
    }
}
