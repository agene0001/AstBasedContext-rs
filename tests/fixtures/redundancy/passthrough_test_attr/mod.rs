//! Regression: a `#[test]` fn whose body is just a single call to one function
//! must NOT be flagged as a passthrough wrapper — a regression test that exists
//! only to exercise a constructor (cf. `dart_parser_constructs_without_panic`)
//! is not redundant indirection. Requires the parser to capture `#[test]` and
//! the passthrough check to skip test fns.

pub fn construct() -> i32 {
    let value = 7;
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_without_panic() {
        construct();
    }
}
