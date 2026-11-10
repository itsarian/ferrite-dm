use crate::model::HashAlgorithm;
use anyhow::Result;
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

pub fn compute_file_hash(path: &Path, algo: HashAlgorithm) -> Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut buf = vec![0u8; 256 * 1024];

    match algo {
        HashAlgorithm::Md5 => {
            let mut hasher = Md5::new();
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            Ok(hex::encode(hasher.finalize()))
        }
        HashAlgorithm::Sha1 => {
            let mut hasher = Sha1::new();
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            Ok(hex::encode(hasher.finalize()))
        }
        HashAlgorithm::Sha256 => {
            let mut hasher = Sha256::new();
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            Ok(hex::encode(hasher.finalize()))
        }
        HashAlgorithm::Sha512 => {
            let mut hasher = Sha512::new();
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            Ok(hex::encode(hasher.finalize()))
        }
    }
}

pub fn verify_file_hash(path: &Path, algo: HashAlgorithm, expected_hex: &str) -> Result<bool> {
    let actual = compute_file_hash(path, algo)?;
    Ok(actual.eq_ignore_ascii_case(expected_hex.trim()))
}

pub fn sha256_str(input: &str) -> String {
    let mut h = Sha256::new();
    h.update(input.as_bytes());
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn hashes_match_known_vectors() {
        let dir = std::env::temp_dir().join(format!("ferrite_hash_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let fpath = dir.join("abc.txt");
        {
            let mut f = File::create(&fpath).unwrap();
            f.write_all(b"abc").unwrap();
        }
        assert_eq!(
            compute_file_hash(&fpath, HashAlgorithm::Md5).unwrap(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            compute_file_hash(&fpath, HashAlgorithm::Sha1).unwrap(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            compute_file_hash(&fpath, HashAlgorithm::Sha256).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(verify_file_hash(
            &fpath,
            HashAlgorithm::Sha256,
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
        )
        .unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
