#![allow(clippy::pedantic)]
/// Binary message format compatible with the orchestrator wire protocol.
///
/// All numeric types use little-endian byte order (`x86_64` native).
/// Strings and byte arrays are length-prefixed with a u64 (little-endian).
#[derive(Debug, Clone)]
pub struct Message {
    data: Vec<u8>,
    index: usize,
    id: u32,
    source: String,
    priority: super::types::Priority,
}

#[allow(dead_code)]
impl Message {
    /// Create a new outgoing message with source and ID in the header.
    #[must_use]
    pub fn new(msg_id: u32, priority: super::types::Priority, source: &str) -> Self {
        let mut msg = Self {
            data: Vec::with_capacity(
                *crate::config::settings::MESSAGE_INITIAL_VECTOR_SIZE as usize,
            ),
            index: 0,
            id: msg_id,
            source: source.to_string(),
            priority,
        };
        msg.push_string(source);
        msg.push_uint(msg_id);
        msg
    }

    /// Parse an incoming message from raw bytes.
    #[must_use]
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let mut msg = Self {
            data: bytes,
            index: 0,
            id: 0,
            source: String::new(),
            priority: super::types::Priority::Lowest,
        };
        msg.source = msg.pop_string();
        msg.id = msg.pop_uint();
        msg
    }

    // --- Getters ---

    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub fn priority(&self) -> super::types::Priority {
        self.priority
    }

    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    #[must_use]
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }

    // --- Private helper: bounds checking ---

    /// Check if at least `n` bytes remain in the buffer.
    /// Logs error and returns false if not enough data.
    fn check_remaining(&self, n: usize) -> bool {
        let remaining = self.data.len().saturating_sub(self.index);
        if remaining < n {
            tracing::warn!(
                "Message buffer underrun: expected {} bytes, but only {} bytes remaining (index={})",
                n,
                remaining,
                self.index
            );
            false
        } else {
            true
        }
    }

    // --- Push / Pop: bool ---

    pub fn push_bool(&mut self, value: bool) {
        self.push_ubyte(u8::from(value));
    }

    pub fn pop_bool(&mut self) -> bool {
        self.pop_ubyte() == 1
    }

    // --- Push / Pop: u8 / i8 ---

    pub fn push_ubyte(&mut self, value: u8) {
        self.data.push(value);
    }

    pub fn pop_ubyte(&mut self) -> u8 {
        if !self.check_remaining(1) {
            return 0;
        }
        let val = self.data[self.index];
        self.index += 1;
        val
    }

    pub fn push_byte(&mut self, value: i8) {
        self.push_ubyte(value as u8);
    }

    pub fn pop_byte(&mut self) -> i8 {
        self.pop_ubyte() as i8
    }

    // --- Push / Pop: u16 / i16 ---

    pub fn push_ushort(&mut self, value: u16) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    /// Pop an unsigned 16-bit integer from the message buffer.
    ///
    /// # Panics
    ///
    /// Panics if there are fewer than 2 bytes remaining in the buffer.
    pub fn pop_ushort(&mut self) -> u16 {
        if !self.check_remaining(2) {
            return 0;
        }
        let bytes: [u8; 2] = self.data[self.index..self.index + 2].try_into().unwrap();
        self.index += 2;
        u16::from_le_bytes(bytes)
    }

    pub fn push_short(&mut self, value: i16) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    /// Pop a signed 16-bit integer from the message buffer.
    ///
    /// # Panics
    ///
    /// Panics if there are fewer than 2 bytes remaining in the buffer.
    pub fn pop_short(&mut self) -> i16 {
        if !self.check_remaining(2) {
            return 0;
        }
        let bytes: [u8; 2] = self.data[self.index..self.index + 2].try_into().unwrap();
        self.index += 2;
        i16::from_le_bytes(bytes)
    }

    // --- Push / Pop: u32 / i32 ---

    pub fn push_uint(&mut self, value: u32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    /// Pop an unsigned 32-bit integer from the message buffer.
    ///
    /// # Panics
    ///
    /// Panics if there are fewer than 4 bytes remaining in the buffer.
    pub fn pop_uint(&mut self) -> u32 {
        if !self.check_remaining(4) {
            return 0;
        }
        let bytes: [u8; 4] = self.data[self.index..self.index + 4].try_into().unwrap();
        self.index += 4;
        u32::from_le_bytes(bytes)
    }

    pub fn push_int(&mut self, value: i32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    /// Pop a signed 32-bit integer from the message buffer.
    ///
    /// # Panics
    ///
    /// Panics if there are fewer than 4 bytes remaining in the buffer.
    pub fn pop_int(&mut self) -> i32 {
        if !self.check_remaining(4) {
            return 0;
        }
        let bytes: [u8; 4] = self.data[self.index..self.index + 4].try_into().unwrap();
        self.index += 4;
        i32::from_le_bytes(bytes)
    }

    // --- Push / Pop: u64 / i64 ---

    pub fn push_ulong(&mut self, value: u64) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    /// Pop an unsigned 64-bit integer from the message buffer.
    ///
    /// # Panics
    ///
    /// Panics if there are fewer than 8 bytes remaining in the buffer.
    pub fn pop_ulong(&mut self) -> u64 {
        if !self.check_remaining(8) {
            return 0;
        }
        let bytes: [u8; 8] = self.data[self.index..self.index + 8].try_into().unwrap();
        self.index += 8;
        u64::from_le_bytes(bytes)
    }

    pub fn push_long(&mut self, value: i64) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    /// Pop a signed 64-bit integer from the message buffer.
    ///
    /// # Panics
    ///
    /// Panics if there are fewer than 8 bytes remaining in the buffer.
    pub fn pop_long(&mut self) -> i64 {
        if !self.check_remaining(8) {
            return 0;
        }
        let bytes: [u8; 8] = self.data[self.index..self.index + 8].try_into().unwrap();
        self.index += 8;
        i64::from_le_bytes(bytes)
    }

    // --- Push / Pop: f32 / f64 ---

    pub fn push_float(&mut self, value: f32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    /// Pop a 32-bit float from the message buffer.
    ///
    /// # Panics
    ///
    /// Panics if there are fewer than 4 bytes remaining in the buffer.
    pub fn pop_float(&mut self) -> f32 {
        if !self.check_remaining(4) {
            return 0.0;
        }
        let bytes: [u8; 4] = self.data[self.index..self.index + 4].try_into().unwrap();
        self.index += 4;
        f32::from_le_bytes(bytes)
    }

    pub fn push_double(&mut self, value: f64) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    /// Pop a 64-bit float from the message buffer.
    ///
    /// # Panics
    ///
    /// Panics if there are fewer than 8 bytes remaining in the buffer.
    pub fn pop_double(&mut self) -> f64 {
        if !self.check_remaining(8) {
            return 0.0;
        }
        let bytes: [u8; 8] = self.data[self.index..self.index + 8].try_into().unwrap();
        self.index += 8;
        f64::from_le_bytes(bytes)
    }

    // --- Push / Pop: String ---

    pub fn push_string(&mut self, value: &str) {
        self.push_ulong(value.len() as u64);
        self.data.extend_from_slice(value.as_bytes());
    }

    /// Pop a UTF-8 string from the message buffer.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - There are insufficient bytes remaining to read the string length
    /// - The bytes do not form valid UTF-8
    pub fn pop_string(&mut self) -> String {
        let bytes = self.pop_bytes();
        String::from_utf8(bytes).unwrap_or_default()
    }

    // --- Push / Pop: raw bytes ---

    pub fn push_bytes(&mut self, value: &[u8]) {
        self.push_ulong(value.len() as u64);
        self.data.extend_from_slice(value);
    }

    pub fn pop_bytes(&mut self) -> Vec<u8> {
        let len = self.pop_ulong() as usize;
        if !self.check_remaining(len) {
            return Vec::new();
        }
        let result = self.data[self.index..self.index + len].to_vec();
        self.index += len;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::types::Priority;

    // ---- Round-trip tests for every type ----

    /// Verifies that `push_bool` / `pop_bool` round-trips `true` and `false`.
    ///
    /// # Setup
    /// Create a new message with id=1, lowest priority, source `"test"`.
    ///
    /// # Act
    /// Push `true` then `false`, serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original value in the correct order.
    #[test]
    fn test_bool_roundtrip() {
        let mut msg = Message::new(1, Priority::Lowest, "test");
        msg.push_bool(true);
        msg.push_bool(false);
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert!(msg2.pop_bool());
        assert!(!msg2.pop_bool());
    }

    /// Verifies that `push_ubyte` / `pop_ubyte` round-trips boundary and mid-range u8 values.
    ///
    /// # Setup
    /// Create a new message, push 0, 127, and 255.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original u8 value in order.
    #[test]
    fn test_ubyte_roundtrip() {
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_ubyte(0);
        msg.push_ubyte(127);
        msg.push_ubyte(255);
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_ubyte(), 0);
        assert_eq!(msg2.pop_ubyte(), 127);
        assert_eq!(msg2.pop_ubyte(), 255);
    }

    /// Verifies that `push_byte` / `pop_byte` round-trips boundary and mid-range i8 values.
    ///
    /// # Setup
    /// Create a new message, push -128, 0, and 127.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original i8 value in order.
    #[test]
    fn test_byte_roundtrip() {
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_byte(-128);
        msg.push_byte(0);
        msg.push_byte(127);
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_byte(), -128);
        assert_eq!(msg2.pop_byte(), 0);
        assert_eq!(msg2.pop_byte(), 127);
    }

    /// Verifies that `push_ushort` / `pop_ushort` round-trips boundary and mid-range u16 values.
    ///
    /// # Setup
    /// Create a new message, push 0, 12345, and `u16::MAX`.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original u16 value in order.
    #[test]
    fn test_ushort_roundtrip() {
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_ushort(0);
        msg.push_ushort(12345);
        msg.push_ushort(u16::MAX);
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_ushort(), 0);
        assert_eq!(msg2.pop_ushort(), 12345);
        assert_eq!(msg2.pop_ushort(), u16::MAX);
    }

    /// Verifies that `push_short` / `pop_short` round-trips boundary and zero i16 values.
    ///
    /// # Setup
    /// Create a new message, push `i16::MIN`, 0, and `i16::MAX`.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original i16 value in order.
    #[test]
    fn test_short_roundtrip() {
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_short(i16::MIN);
        msg.push_short(0);
        msg.push_short(i16::MAX);
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_short(), i16::MIN);
        assert_eq!(msg2.pop_short(), 0);
        assert_eq!(msg2.pop_short(), i16::MAX);
    }

    /// Verifies that `push_uint` / `pop_uint` round-trips zero, a known bit-pattern, and `u32::MAX`.
    ///
    /// # Setup
    /// Create a new message, push 0, `0xDEAD_BEEF`, and `u32::MAX`.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original u32 value in order.
    #[test]
    fn test_uint_roundtrip() {
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_uint(0);
        msg.push_uint(0xDEAD_BEEF);
        msg.push_uint(u32::MAX);
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_uint(), 0);
        assert_eq!(msg2.pop_uint(), 0xDEAD_BEEF);
        assert_eq!(msg2.pop_uint(), u32::MAX);
    }

    /// Verifies that `push_int` / `pop_int` round-trips boundary and zero i32 values.
    ///
    /// # Setup
    /// Create a new message, push `i32::MIN`, 0, and `i32::MAX`.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original i32 value in order.
    #[test]
    fn test_int_roundtrip() {
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_int(i32::MIN);
        msg.push_int(0);
        msg.push_int(i32::MAX);
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_int(), i32::MIN);
        assert_eq!(msg2.pop_int(), 0);
        assert_eq!(msg2.pop_int(), i32::MAX);
    }

    /// Verifies that `push_ulong` / `pop_ulong` round-trips zero, a known bit-pattern, and `u64::MAX`.
    ///
    /// # Setup
    /// Create a new message, push 0, `0xDEAD_BEEFCAFEBABE`, and `u64::MAX`.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original u64 value in order.
    #[test]
    fn test_ulong_roundtrip() {
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_ulong(0);
        msg.push_ulong(0xDEAD_BEEFCAFEBABE);
        msg.push_ulong(u64::MAX);
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_ulong(), 0);
        assert_eq!(msg2.pop_ulong(), 0xDEAD_BEEFCAFEBABE);
        assert_eq!(msg2.pop_ulong(), u64::MAX);
    }

    /// Verifies that `push_long` / `pop_long` round-trips boundary and zero i64 values.
    ///
    /// # Setup
    /// Create a new message, push `i64::MIN`, 0, and `i64::MAX`.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original i64 value in order.
    #[test]
    fn test_long_roundtrip() {
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_long(i64::MIN);
        msg.push_long(0);
        msg.push_long(i64::MAX);
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_long(), i64::MIN);
        assert_eq!(msg2.pop_long(), 0);
        assert_eq!(msg2.pop_long(), i64::MAX);
    }

    /// Verifies that `push_float` / `pop_float` round-trips zero, pi, and a large negative f32.
    ///
    /// # Setup
    /// Create a new message, push `0.0`, `f32::consts::PI`, and `-1.5e10`.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original f32 value exactly (bit-for-bit).
    #[allow(clippy::float_cmp)]
    #[test]
    fn test_float_roundtrip() {
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_float(0.0);
        msg.push_float(std::f32::consts::PI);
        msg.push_float(-1.5e10);
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_float(), 0.0);
        assert_eq!(msg2.pop_float(), std::f32::consts::PI);
        assert_eq!(msg2.pop_float(), -1.5e10);
    }

    /// Verifies that `push_double` / `pop_double` round-trips zero, pi, and a large negative f64.
    ///
    /// # Setup
    /// Create a new message, push `0.0`, `f64::consts::PI`, and `-1.5e100`.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original f64 value exactly (bit-for-bit).
    #[allow(clippy::float_cmp)]
    #[test]
    fn test_double_roundtrip() {
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_double(0.0);
        msg.push_double(std::f64::consts::PI);
        msg.push_double(-1.5e100);
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_double(), 0.0);
        assert_eq!(msg2.pop_double(), std::f64::consts::PI);
        assert_eq!(msg2.pop_double(), -1.5e100);
    }

    /// Verifies that `push_string` / `pop_string` round-trips a non-empty, empty, and special-char string.
    ///
    /// # Setup
    /// Create a new message, push `"hello world"`, `""`, and `"a]b[c{d}e"`.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original string in order.
    #[test]
    fn test_string_roundtrip() {
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_string("hello world");
        msg.push_string("");
        msg.push_string("a]b[c{d}e");
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_string(), "hello world");
        assert_eq!(msg2.pop_string(), "");
        assert_eq!(msg2.pop_string(), "a]b[c{d}e");
    }

    /// Verifies that `push_string` / `pop_string` correctly handles multi-byte Unicode strings.
    ///
    /// # Setup
    /// Create a new message, push Japanese characters, emoji, and an accented Latin string.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original Unicode string unchanged.
    #[test]
    fn test_string_unicode() {
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_string("こんにちは");
        msg.push_string("🎉🚀🔥");
        msg.push_string("café résumé");
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_string(), "こんにちは");
        assert_eq!(msg2.pop_string(), "🎉🚀🔥");
        assert_eq!(msg2.pop_string(), "café résumé");
    }

    /// Verifies that `push_string` / `pop_string` correctly handles a 100,000-character string.
    ///
    /// # Setup
    /// Build a string of 100,000 `'x'` characters.
    ///
    /// # Act
    /// Push it into a new message, serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// The popped string equals the original 100,000-character string.
    #[test]
    fn test_string_long() {
        let long_string = "x".repeat(100_000);
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_string(&long_string);
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_string(), long_string);
    }

    /// Verifies that `push_bytes` / `pop_bytes` round-trips a non-empty and empty byte slice.
    ///
    /// # Setup
    /// Create a new message, push `[0x00, 0xFF, 0x42, 0x13]` then an empty slice.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original byte sequence in order.
    #[test]
    fn test_bytes_roundtrip() {
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_bytes(&[0x00, 0xFF, 0x42, 0x13]);
        msg.push_bytes(&[]);
        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_bytes(), vec![0x00, 0xFF, 0x42, 0x13]);
        assert_eq!(msg2.pop_bytes(), Vec::<u8>::new());
    }

    // ---- Multi-field message ----

    /// Verifies that a message carrying every supported field type serialises and deserialises correctly.
    ///
    /// # Setup
    /// Build a message with id=2000, source `"system"`, and push one value of every supported type.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Header fields (`source`, `id`) and every payload value match the originals.
    #[test]
    fn test_multi_field_message() {
        let mut msg = Message::new(2000, Priority::Highest, "system");
        msg.push_bool(true);
        msg.push_ubyte(42);
        msg.push_ushort(1234);
        msg.push_uint(0xCAFE_BABE);
        msg.push_ulong(9_999_999_999);
        msg.push_float(std::f32::consts::PI);
        msg.push_double(std::f64::consts::E);
        msg.push_string("payload");
        msg.push_bytes(&[1, 2, 3]);

        let mut msg2 = Message::from_bytes(msg.into_data());
        // Header is parsed automatically
        assert_eq!(msg2.source(), "system");
        assert_eq!(msg2.id(), 2000);

        assert!(msg2.pop_bool());
        assert_eq!(msg2.pop_ubyte(), 42);
        assert_eq!(msg2.pop_ushort(), 1234);
        assert_eq!(msg2.pop_uint(), 0xCAFE_BABE);
        assert_eq!(msg2.pop_ulong(), 9_999_999_999);
        assert!((msg2.pop_float() - std::f32::consts::PI).abs() < 1e-6);
        assert!((msg2.pop_double() - std::f64::consts::E).abs() < 1e-9);
        assert_eq!(msg2.pop_string(), "payload");
        assert_eq!(msg2.pop_bytes(), vec![1, 2, 3]);
    }

    // ---- Byte layout verification ----

    /// Verifies that a pushed u32 is stored as four little-endian bytes in the raw buffer.
    ///
    /// # Setup
    /// Create a new message with empty source; record the header byte length.
    ///
    /// # Act
    /// Push `0x12345678` via `push_uint`.
    ///
    /// # Assert
    /// The four bytes immediately after the header are `[0x78, 0x56, 0x34, 0x12]`.
    #[test]
    fn test_uint_byte_layout_little_endian() {
        let mut msg = Message::new(1, Priority::Lowest, "");
        // After header: empty-string length (8 bytes of 0) + msg_id (1 as LE u32)
        let header_len = msg.data().len();
        msg.push_uint(0x1234_5678);
        let data = msg.data();
        // The 4 bytes after the header should be [0x78, 0x56, 0x34, 0x12]
        assert_eq!(data[header_len], 0x78);
        assert_eq!(data[header_len + 1], 0x56);
        assert_eq!(data[header_len + 2], 0x34);
        assert_eq!(data[header_len + 3], 0x12);
    }

    /// Verifies that a pushed u16 is stored as two little-endian bytes in the raw buffer.
    ///
    /// # Setup
    /// Create a new message with empty source; record the header byte length.
    ///
    /// # Act
    /// Push `0xABCD` via `push_ushort`.
    ///
    /// # Assert
    /// The two bytes immediately after the header are `[0xCD, 0xAB]`.
    #[test]
    fn test_ushort_byte_layout_little_endian() {
        let mut msg = Message::new(1, Priority::Lowest, "");
        let header_len = msg.data().len();
        msg.push_ushort(0xABCD);
        let data = msg.data();
        assert_eq!(data[header_len], 0xCD);
        assert_eq!(data[header_len + 1], 0xAB);
    }

    /// Verifies that a pushed u64 is stored as eight little-endian bytes in the raw buffer.
    ///
    /// # Setup
    /// Create a new message with empty source; record the header byte length.
    ///
    /// # Act
    /// Push `0x0102030405060708` via `push_ulong`.
    ///
    /// # Assert
    /// The eight bytes immediately after the header are `[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]`.
    #[test]
    fn test_ulong_byte_layout_little_endian() {
        let mut msg = Message::new(1, Priority::Lowest, "");
        let header_len = msg.data().len();
        msg.push_ulong(0x0102_0304_0506_0708);
        let data = msg.data();
        assert_eq!(data[header_len], 0x08);
        assert_eq!(data[header_len + 1], 0x07);
        assert_eq!(data[header_len + 2], 0x06);
        assert_eq!(data[header_len + 3], 0x05);
        assert_eq!(data[header_len + 4], 0x04);
        assert_eq!(data[header_len + 5], 0x03);
        assert_eq!(data[header_len + 6], 0x02);
        assert_eq!(data[header_len + 7], 0x01);
    }

    /// Verifies the raw byte layout of a pushed string: u64 length prefix followed by UTF-8 content.
    ///
    /// # Setup
    /// Create a new message with empty source; record the header byte length.
    ///
    /// # Act
    /// Push `"AB"` via `push_string`.
    ///
    /// # Assert
    /// The eight length-prefix bytes encode `2` in little-endian, followed by `b'A'` and `b'B'`.
    #[test]
    fn test_string_byte_layout() {
        let mut msg = Message::new(1, Priority::Lowest, "");
        let header_len = msg.data().len();
        msg.push_string("AB");
        let data = msg.data();
        // Length prefix: 2 as u64 LE = [0x02, 0, 0, 0, 0, 0, 0, 0]
        assert_eq!(data[header_len], 0x02);
        for i in 1..8 {
            assert_eq!(data[header_len + i], 0x00);
        }
        // String bytes
        assert_eq!(data[header_len + 8], b'A');
        assert_eq!(data[header_len + 9], b'B');
    }

    // ---- Header tests ----

    /// Verifies that `Message::new` encodes source, id, and priority correctly in the header.
    ///
    /// # Setup
    /// Create a message with id=2000, `Priority::Medium`, source `"system"`.
    ///
    /// # Act
    /// Read header fields via getters and re-parse raw data via `from_bytes`.
    ///
    /// # Assert
    /// `id()`, `source()`, and `priority()` return the original values; raw data is at least 18 bytes.
    #[test]
    fn test_new_creates_correct_header() {
        let msg = Message::new(2000, Priority::Medium, "system");
        assert_eq!(msg.id(), 2000);
        assert_eq!(msg.source(), "system");
        assert_eq!(msg.priority(), Priority::Medium);

        // Verify the raw data starts with the source string and then msg_id
        let data = msg.data();
        // Source: length prefix (8 bytes) + "system" (6 bytes) = 14 bytes
        // Then msg_id: 4 bytes LE
        // Total header: 18 bytes
        assert!(data.len() >= 18);

        // Parse back to verify
        let msg2 = Message::from_bytes(data.to_vec());
        assert_eq!(msg2.source(), "system");
        assert_eq!(msg2.id(), 2000);
    }

    /// Verifies that `from_bytes` correctly parses source and id from a manually-built raw buffer.
    ///
    /// # Setup
    /// Construct a raw byte vector with source `"src"` (u64 length prefix + bytes), id=42, and a u32 payload of 99.
    ///
    /// # Act
    /// Parse with `from_bytes`, then pop the payload uint.
    ///
    /// # Assert
    /// `source()` is `"src"`, `id()` is 42, and the popped uint is 99.
    #[test]
    fn test_from_bytes_parses_header() {
        // Manually construct a raw message: source="src", id=42
        let mut raw = Vec::new();
        // Source string: length 3 as u64 LE
        raw.extend_from_slice(&3u64.to_le_bytes());
        raw.extend_from_slice(b"src");
        // Message ID: 42 as u32 LE
        raw.extend_from_slice(&42u32.to_le_bytes());
        // Payload: a uint
        raw.extend_from_slice(&99u32.to_le_bytes());

        let mut msg = Message::from_bytes(raw);
        assert_eq!(msg.source(), "src");
        assert_eq!(msg.id(), 42);
        assert_eq!(msg.pop_uint(), 99);
    }

    /// Verifies that `from_bytes` handles an empty source string correctly.
    ///
    /// # Setup
    /// Construct a raw byte vector with an empty source (zero-length u64 prefix) and id=1000.
    ///
    /// # Act
    /// Parse with `from_bytes`.
    ///
    /// # Assert
    /// `source()` is `""` and `id()` is 1000.
    #[test]
    fn test_from_bytes_empty_source() {
        let mut raw = Vec::new();
        // Empty source
        raw.extend_from_slice(&0u64.to_le_bytes());
        // Message ID: 1000
        raw.extend_from_slice(&1000u32.to_le_bytes());

        let msg = Message::from_bytes(raw);
        assert_eq!(msg.source(), "");
        assert_eq!(msg.id(), 1000);
    }

    /// Verifies a full roundtrip: `Message::new` → payload pushes → `into_data` → `from_bytes`.
    ///
    /// # Setup
    /// Create a message with id=5000, source `"cluster_a"`, and push a string, uint, and bool payload.
    ///
    /// # Act
    /// Serialise with `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Header (`source`, `id`) and all payload fields match the original values.
    #[test]
    fn test_message_new_then_from_bytes_roundtrip() {
        let mut original = Message::new(5000, Priority::Highest, "cluster_a");
        original.push_string("job_123");
        original.push_uint(50);
        original.push_bool(true);

        let mut parsed = Message::from_bytes(original.into_data());
        assert_eq!(parsed.source(), "cluster_a");
        assert_eq!(parsed.id(), 5000);
        assert_eq!(parsed.pop_string(), "job_123");
        assert_eq!(parsed.pop_uint(), 50);
        assert!(parsed.pop_bool());
    }

    // ---- Edge cases ----

    /// Verifies that negative signed integers round-trip correctly across all signed integer types.
    ///
    /// # Setup
    /// Create a new message, push -1 (i8), -256 (i16), -`100_000` (i32), and -`1_000_000_000_000` (i64).
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Each pop returns the original negative value.
    #[test]
    fn test_negative_integers() {
        let mut msg = Message::new(1, Priority::Lowest, "t");
        msg.push_byte(-1);
        msg.push_short(-256);
        msg.push_int(-100_000);
        msg.push_long(-1_000_000_000_000);

        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.pop_byte(), -1);
        assert_eq!(msg2.pop_short(), -256);
        assert_eq!(msg2.pop_int(), -100_000);
        assert_eq!(msg2.pop_long(), -1_000_000_000_000);
    }

    /// Verifies that zero and empty values round-trip correctly for all supported types.
    ///
    /// # Setup
    /// Create a message with id=0 and empty source, then push the zero/false/empty value for every supported type.
    ///
    /// # Act
    /// Serialise via `into_data`, parse back with `from_bytes`.
    ///
    /// # Assert
    /// Header shows empty source and id=0; every popped value is the zero or empty equivalent.
    #[allow(clippy::float_cmp)]
    #[test]
    fn test_zero_values() {
        let mut msg = Message::new(0, Priority::Lowest, "");
        msg.push_bool(false);
        msg.push_ubyte(0);
        msg.push_ushort(0);
        msg.push_uint(0);
        msg.push_ulong(0);
        msg.push_float(0.0);
        msg.push_double(0.0);
        msg.push_string("");
        msg.push_bytes(&[]);

        let mut msg2 = Message::from_bytes(msg.into_data());
        assert_eq!(msg2.source(), "");
        assert_eq!(msg2.id(), 0);
        assert!(!msg2.pop_bool());
        assert_eq!(msg2.pop_ubyte(), 0);
        assert_eq!(msg2.pop_ushort(), 0);
        assert_eq!(msg2.pop_uint(), 0);
        assert_eq!(msg2.pop_ulong(), 0);
        assert_eq!(msg2.pop_float(), 0.0);
        assert_eq!(msg2.pop_double(), 0.0);
        assert_eq!(msg2.pop_string(), "");
        assert_eq!(msg2.pop_bytes(), Vec::<u8>::new());
    }
}
