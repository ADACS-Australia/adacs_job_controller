/// Generate a new random UUID v4 string.
#[must_use]
pub fn generate_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Base64-encode raw bytes using the standard alphabet.
#[allow(dead_code)]
#[must_use]
pub fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Base64-decode a string using the standard alphabet.
///
/// # Errors
///
/// Returns an error if the input is not valid base64.
#[allow(dead_code)]
pub fn base64_decode(data: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_uuid_format() {
        let id = generate_uuid();
        // UUID v4 format: 8-4-4-4-12 hex chars
        assert_eq!(id.len(), 36);
        assert_eq!(id.chars().filter(|&c| c == '-').count(), 4);
    }

    #[test]
    fn test_generate_uuid_uniqueness() {
        let a = generate_uuid();
        let b = generate_uuid();
        assert_ne!(a, b);
    }

    #[test]
    fn test_base64_encode_empty() {
        assert_eq!(base64_encode(&[]), "");
    }

    #[test]
    fn test_base64_encode_decode_roundtrip() {
        let data = b"Hello, World!";
        let encoded = base64_encode(data);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_base64_known_values() {
        assert_eq!(base64_encode(b"Hello"), "SGVsbG8=");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"abc"), "YWJj");
    }

    #[test]
    fn test_base64_decode_invalid() {
        assert!(base64_decode("!!!invalid!!!").is_err());
    }

    #[test]
    fn test_base64_binary_data() {
        let data: Vec<u8> = (0..=255).collect();
        let encoded = base64_encode(&data);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }
}
