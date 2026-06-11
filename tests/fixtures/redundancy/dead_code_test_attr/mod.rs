//! Regression: a `#[test]` fn inside a `#[cfg(test)]` module has no callers,
//! but must NOT be reported as dead code. This only works if the Rust parser
//! captures the `#[test]` attribute into `decorators` (otherwise the dead-code
//! check can't tell it's a test). Name deliberately does not start with "test"
//! so the name heuristic doesn't mask the real fix.

pub fn real_entry() -> i32 {
    helper_value() + 1
}

fn helper_value() -> i32 {
    let base = 40;
    let bonus = 2;
    base + bonus
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_entry_sums_correctly() {
        let got = real_entry();
        let want = 43;
        assert_eq!(got, want);
    }
}
