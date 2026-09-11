/// The identity of the extension the editor is running.
///
/// An explicit `AURUM_RUNTIME_FINGERPRINT` wins, which is what the acceptance
/// test uses to drive a known sequence of values. Otherwise the identity comes
/// from the build script, which derives it from the sources this library was
/// compiled from — so it moves when the code moves and stays put when it does
/// not.
///
/// That distinction is what lets `aurum dev` prove a reload rather than assume
/// one: a fingerprint that changed between builds is evidence the running code
/// changed, and until this existed there was no per-build value to change.
pub(crate) fn runtime_fingerprint() -> &'static str {
    option_env!("AURUM_RUNTIME_FINGERPRINT")
        .filter(|value| !value.is_empty())
        .unwrap_or(env!("AURUM_BUILD_ID"))
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
            // No explicit fingerprint, so the build script's identity is what
            // the editor will report. Asserting the shape rather than the
            // value, because the value is meant to change with the sources.
            _ => {
                let fingerprint = runtime_fingerprint();
                assert!(
                    fingerprint.starts_with("build-"),
                    "the derived identity should be recognisable, got '{fingerprint}'"
                );
                assert_eq!(
                    fingerprint.len(),
                    "build-".len() + 16,
                    "the derived identity should be a fixed width, got '{fingerprint}'"
                );
            }
        }
    }
}
