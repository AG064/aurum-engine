pub(crate) fn runtime_fingerprint() -> &'static str {
    option_env!("AURUM_RUNTIME_FINGERPRINT")
        .filter(|value| !value.is_empty())
        .unwrap_or("aurum-unmanaged")
}

#[cfg(test)]
mod tests {
    use super::runtime_fingerprint;

    #[test]
    fn runtime_fingerprint_is_never_empty() {
        assert!(!runtime_fingerprint().is_empty());
    }

    #[test]
    fn runtime_fingerprint_matches_compile_time_configuration() {
        match option_env!("AURUM_RUNTIME_FINGERPRINT") {
            Some(value) if !value.is_empty() => assert_eq!(runtime_fingerprint(), value),
            _ => assert_eq!(runtime_fingerprint(), "aurum-unmanaged"),
        }
    }
}
