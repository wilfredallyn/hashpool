//! Share validation utilities for minimum difficulty checks.

/// Counts the number of leading zero bits in a 32-byte hash.
///
/// This function iterates through the hash bytes from the beginning and counts
/// consecutive zero bits. It stops at the first non-zero bit.
///
/// # Arguments
/// * `hash` - A 32-byte hash (typically a share hash)
///
/// # Returns
/// The number of leading zero bits (0-256)
pub fn count_leading_zero_bits(hash: &[u8; 32]) -> u32 {
    let mut zero_bits = 0u32;

    for &byte in hash {
        if byte == 0 {
            // All 8 bits are zero
            zero_bits += 8;
        } else {
            // Count leading zero bits in this byte and add to total
            // For u8, leading_zeros() counts leading zeros in 8-bit representation
            zero_bits += byte.leading_zeros() as u32;
            break;
        }
    }

    zero_bits
}

/// Validates that a share meets the minimum difficulty threshold.
///
/// Returns `Ok(())` if the share has at least `minimum_bits` leading zero bits,
/// or if `minimum_bits` is `None` (no constraint).
///
/// Returns `Err()` if the share has fewer than `minimum_bits` leading zero bits.
///
/// # Arguments
/// * `hash` - The share hash to validate
/// * `minimum_bits` - Optional minimum leading zero bits required
///
/// # Returns
/// `Ok(())` if the share meets the threshold, `Err(String)` otherwise
pub fn validate_share_difficulty(hash: &[u8; 32], minimum_bits: Option<u32>) -> Result<(), String> {
    if let Some(min_bits) = minimum_bits {
        let leading_zeros = count_leading_zero_bits(hash);
        if leading_zeros < min_bits {
            return Err(format!(
                "Share difficulty too low: {} leading zero bits (minimum: {})",
                leading_zeros, min_bits
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_leading_zero_bits() {
        // Hash with 32 leading zero bits
        let hash_32_zeros = [
            0x00, 0x00, 0x00, 0x00, // 32 zero bits
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
        ];
        assert_eq!(count_leading_zero_bits(&hash_32_zeros), 32);

        // Hash with 24 leading zero bits (0x00, 0x00, 0x00, 0x0F = 24 leading zeros in first 4 bytes)
        let hash_24_zeros = [
            0x00, 0x00, 0x00, 0x0F,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
        ];
        assert_eq!(count_leading_zero_bits(&hash_24_zeros), 28);

        // Hash with 0 leading zero bits
        let hash_no_zeros = [
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
        ];
        assert_eq!(count_leading_zero_bits(&hash_no_zeros), 0);
    }

    #[test]
    fn test_validate_share_difficulty() {
        let hash_32_zeros = [
            0x00, 0x00, 0x00, 0x00,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF,
        ];

        // Should pass with no constraint
        assert!(validate_share_difficulty(&hash_32_zeros, None).is_ok());

        // Should pass with constraint <= 32
        assert!(validate_share_difficulty(&hash_32_zeros, Some(32)).is_ok());
        assert!(validate_share_difficulty(&hash_32_zeros, Some(16)).is_ok());

        // Should fail with constraint > 32
        assert!(validate_share_difficulty(&hash_32_zeros, Some(40)).is_err());
    }
}
