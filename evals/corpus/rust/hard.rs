macro_rules! my_macro {
    () => {};
}

pub fn long_signature(
    a: u32,
    b: u32,
) -> u32 {
    a + b
}

fn outer() {
    fn inner() -> u32 {
        42
    }
    let _ = inner();
}
