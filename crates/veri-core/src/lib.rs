pub mod config;

pub fn hello() -> &'static str {
    "veri-core: hello"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello() {
        assert_eq!(hello(), "veri-core: hello");
    }
}
