//! Room codes: 5 uppercase letters, no vowels, never `O` or `I`.

use rand_core::Rng;

/// Letters allowed in a room code. Uppercase consonants only, so codes never
/// spell words and never contain `O`, `I`, `0` or `1`.
pub const ROOM_CODE_ALPHABET: &[u8; 21] = b"BCDFGHJKLMNPQRSTVWXYZ";

/// Number of characters in a room code.
pub const ROOM_CODE_LEN: usize = 5;

/// Generate a room code from a random source.
pub fn generate_code<R: Rng + ?Sized>(rng: &mut R) -> String {
    (0..ROOM_CODE_LEN)
        .map(|_| {
            let i = (rng.next_u32() as usize) % ROOM_CODE_ALPHABET.len();
            ROOM_CODE_ALPHABET[i] as char
        })
        .collect()
}

/// Normalize user input (trim, uppercase) and check it is a valid room code.
pub fn parse_code(input: &str) -> Option<String> {
    let code = input.trim().to_ascii_uppercase();
    let valid = code.len() == ROOM_CODE_LEN && code.bytes().all(|b| ROOM_CODE_ALPHABET.contains(&b));
    valid.then_some(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rand_chacha::ChaCha20Rng;
    use rand_core::SeedableRng;

    #[test]
    fn alphabet_has_no_vowels_or_confusables() {
        for b in ROOM_CODE_ALPHABET {
            assert!(!b"AEIOU01".contains(b), "{} not allowed", *b as char);
        }
    }

    #[test]
    fn parse_normalizes_case_and_space() {
        assert_eq!(parse_code(" bcdfg "), Some("BCDFG".to_string()));
        assert_eq!(parse_code("BCDFA"), None);
        assert_eq!(parse_code("BCDF"), None);
        assert_eq!(parse_code("BCDFGH"), None);
    }

    proptest! {
        #[test]
        fn generated_codes_always_parse(seed in any::<u64>()) {
            let mut rng = ChaCha20Rng::seed_from_u64(seed);
            let code = generate_code(&mut rng);
            prop_assert_eq!(parse_code(&code), Some(code.clone()));
        }
    }
}
