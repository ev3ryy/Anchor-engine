pub const MAGIC: u32 = 0x0723_0203;

pub fn spirv_words(bytes: &[u8]) -> Result<Vec<u32>, &'static str> {
    if bytes.len() % 4 != 0 {
        return Err("SPIR-V length is not a multiple of 4");
    }
    if bytes.is_empty() {
        return Err("SPIR-V is empty");
    }
    let mut words = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(chunk);
        words.push(u32::from_le_bytes(bytes));
    }
    if words[0] != MAGIC {
        return Err("invalid SPIR-V magic");
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::{MAGIC, spirv_words};

    #[test]
    fn rejects_a_length_that_is_not_a_multiple_of_four() {
        let error = spirv_words(&[0x03, 0x02, 0x23]).expect_err("odd length");
        assert!(error.contains("multiple of 4"), "{error}");
    }

    #[test]
    fn copies_bytes_into_aligned_words() {
        let mut bytes = MAGIC.to_le_bytes().to_vec();
        bytes.extend_from_slice(&7u32.to_le_bytes());
        assert_eq!(spirv_words(&bytes).expect("words"), vec![MAGIC, 7]);
    }

    #[test]
    fn rejects_a_bad_magic() {
        let bytes = 0x1122_3344u32.to_le_bytes();
        assert!(spirv_words(&bytes).is_err());
    }
}
