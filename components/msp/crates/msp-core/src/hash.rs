use crate::{MspError, MspResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha384, Sha512};
use std::{fmt, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgorithm {
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlgorithm {
    pub fn as_prefix(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
            Self::Sha384 => "sha384",
            Self::Sha512 => "sha512",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HashDigest {
    pub algorithm: HashAlgorithm,
    pub hex: String,
}

impl HashDigest {
    pub fn parse(value: &str) -> MspResult<Self> {
        let (prefix, hex_value) = value
            .split_once(':')
            .ok_or_else(|| MspError::InvalidHash(value.to_string()))?;
        let algorithm = match prefix {
            "sha256" => HashAlgorithm::Sha256,
            "sha384" => HashAlgorithm::Sha384,
            "sha512" => HashAlgorithm::Sha512,
            _ => return Err(MspError::InvalidHash(value.to_string())),
        };
        if hex_value.is_empty() || hex::decode(hex_value).is_err() {
            return Err(MspError::InvalidHash(value.to_string()));
        }
        Ok(Self {
            algorithm,
            hex: hex_value.to_ascii_lowercase(),
        })
    }

    pub fn from_bytes(algorithm: HashAlgorithm, bytes: &[u8]) -> Self {
        let hex = match algorithm {
            HashAlgorithm::Sha256 => hex::encode(Sha256::digest(bytes)),
            HashAlgorithm::Sha384 => hex::encode(Sha384::digest(bytes)),
            HashAlgorithm::Sha512 => hex::encode(Sha512::digest(bytes)),
        };
        Self { algorithm, hex }
    }

    pub fn from_file(path: impl AsRef<Path>, algorithm: HashAlgorithm) -> MspResult<Self> {
        let bytes = std::fs::read(path)?;
        Ok(Self::from_bytes(algorithm, &bytes))
    }
}

impl fmt::Display for HashDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algorithm.as_prefix(), self.hex)
    }
}

impl TryFrom<String> for HashDigest {
    type Error = MspError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<HashDigest> for String {
    fn from(value: HashDigest) -> Self {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_hash() {
        let digest = HashDigest::parse("sha256:abcd").unwrap();
        assert_eq!(digest.algorithm, HashAlgorithm::Sha256);
        assert_eq!(digest.to_string(), "sha256:abcd");
    }

    #[test]
    fn rejects_invalid_hash() {
        assert!(HashDigest::parse("md5:abcd").is_err());
        assert!(HashDigest::parse("sha256:not-hex").is_err());
    }
}
