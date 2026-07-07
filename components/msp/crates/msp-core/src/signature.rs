use crate::{HashAlgorithm, HashDigest, MspError, MspResult};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::{fmt, path::Path, str::FromStr};

const ED25519_PUBLIC_KEY_LEN: usize = 32;
const ED25519_SIGNATURE_LEN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureAlgorithm {
    Ed25519,
}

impl SignatureAlgorithm {
    pub fn as_policy_name(self) -> &'static str {
        match self {
            Self::Ed25519 => "ed25519",
        }
    }
}

impl fmt::Display for SignatureAlgorithm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_policy_name())
    }
}

impl FromStr for SignatureAlgorithm {
    type Err = MspError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ed25519" => Ok(Self::Ed25519),
            other => Err(MspError::Signature(format!(
                "unsupported signature algorithm: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedSignature {
    pub algorithm: SignatureAlgorithm,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

impl ParsedSignature {
    pub fn public_key_ref(&self) -> String {
        match self.algorithm {
            SignatureAlgorithm::Ed25519 => format!("ed25519:{}", STANDARD.encode(&self.public_key)),
        }
    }

    pub fn public_key_sha256_ref(&self) -> String {
        HashDigest::from_bytes(HashAlgorithm::Sha256, &self.public_key).to_string()
    }

    pub fn public_key_matches_ref(&self, expected: &str) -> bool {
        expected == self.public_key_ref() || expected == self.public_key_sha256_ref()
    }
}

impl ParsedSignature {
    pub fn parse(value: &str) -> MspResult<Self> {
        let mut parts = value.split(':');
        let algorithm = parts
            .next()
            .ok_or_else(|| MspError::Signature("missing signature algorithm".to_string()))?
            .parse()?;
        let public_key = parts
            .next()
            .ok_or_else(|| MspError::Signature("missing signature public key".to_string()))?;
        let signature = parts
            .next()
            .ok_or_else(|| MspError::Signature("missing signature bytes".to_string()))?;
        if parts.next().is_some() {
            return Err(MspError::Signature(
                "signature must have format ed25519:<base64-public-key>:<base64-signature>"
                    .to_string(),
            ));
        }

        let public_key = STANDARD.decode(public_key).map_err(|error| {
            MspError::Signature(format!("invalid signature public key: {error}"))
        })?;
        let signature = STANDARD
            .decode(signature)
            .map_err(|error| MspError::Signature(format!("invalid signature bytes: {error}")))?;

        if public_key.len() != ED25519_PUBLIC_KEY_LEN {
            return Err(MspError::Signature(format!(
                "ed25519 public key must be {ED25519_PUBLIC_KEY_LEN} bytes, got {}",
                public_key.len()
            )));
        }
        if signature.len() != ED25519_SIGNATURE_LEN {
            return Err(MspError::Signature(format!(
                "ed25519 signature must be {ED25519_SIGNATURE_LEN} bytes, got {}",
                signature.len()
            )));
        }

        Ok(Self {
            algorithm,
            public_key,
            signature,
        })
    }

    pub fn encode_ed25519(public_key: &[u8], signature: &[u8]) -> MspResult<String> {
        if public_key.len() != ED25519_PUBLIC_KEY_LEN {
            return Err(MspError::Signature(format!(
                "ed25519 public key must be {ED25519_PUBLIC_KEY_LEN} bytes, got {}",
                public_key.len()
            )));
        }
        if signature.len() != ED25519_SIGNATURE_LEN {
            return Err(MspError::Signature(format!(
                "ed25519 signature must be {ED25519_SIGNATURE_LEN} bytes, got {}",
                signature.len()
            )));
        }
        Ok(format!(
            "ed25519:{}:{}",
            STANDARD.encode(public_key),
            STANDARD.encode(signature)
        ))
    }
}

/// Sign content with a raw 32-byte Ed25519 signing seed and encode the
/// detached signature as `ed25519:<base64-public-key>:<base64-signature>`.
pub fn sign_ed25519_bytes(content: &[u8], signing_seed: &[u8]) -> MspResult<String> {
    let seed: [u8; ED25519_PUBLIC_KEY_LEN] = signing_seed.try_into().map_err(|_| {
        MspError::Signature(format!(
            "ed25519 signing seed must be {ED25519_PUBLIC_KEY_LEN} bytes, got {}",
            signing_seed.len()
        ))
    })?;
    let signing_key = SigningKey::from_bytes(&seed);
    let signature = signing_key.sign(content);
    ParsedSignature::encode_ed25519(
        signing_key.verifying_key().as_bytes(),
        &signature.to_bytes(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureVerifyResult {
    pub artifact: String,
    pub signed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<SignatureAlgorithm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key_sha256: Option<String>,
    pub passed: bool,
    pub reasons: Vec<String>,
}

pub fn verify_signature_bytes(
    artifact: impl Into<String>,
    content: &[u8],
    signed: bool,
    signature: Option<&str>,
    allowed_algorithms: &[String],
) -> SignatureVerifyResult {
    let artifact = artifact.into();
    let mut reasons = Vec::new();

    if !signed {
        reasons.push("artifact is marked unsigned".to_string());
        return SignatureVerifyResult {
            artifact,
            signed,
            algorithm: None,
            public_key_ref: None,
            public_key_sha256: None,
            passed: false,
            reasons,
        };
    }

    let Some(signature) = signature else {
        reasons.push("artifact is marked signed but no signature is present".to_string());
        return SignatureVerifyResult {
            artifact,
            signed,
            algorithm: None,
            public_key_ref: None,
            public_key_sha256: None,
            passed: false,
            reasons,
        };
    };

    let parsed = match ParsedSignature::parse(signature) {
        Ok(parsed) => parsed,
        Err(error) => {
            reasons.push(error.to_string());
            return SignatureVerifyResult {
                artifact,
                signed,
                algorithm: None,
                public_key_ref: None,
                public_key_sha256: None,
                passed: false,
                reasons,
            };
        }
    };

    if !allowed_algorithms.is_empty()
        && !allowed_algorithms
            .iter()
            .any(|algorithm| algorithm == parsed.algorithm.as_policy_name())
    {
        reasons.push(format!(
            "signature algorithm {} is not allowed by policy",
            parsed.algorithm
        ));
        return SignatureVerifyResult {
            artifact,
            signed,
            algorithm: Some(parsed.algorithm),
            public_key_ref: Some(parsed.public_key_ref()),
            public_key_sha256: Some(parsed.public_key_sha256_ref()),
            passed: false,
            reasons,
        };
    }

    let public_key_bytes: [u8; ED25519_PUBLIC_KEY_LEN] = parsed
        .public_key
        .as_slice()
        .try_into()
        .expect("parsed ed25519 public key length is checked");
    let signature_bytes: [u8; ED25519_SIGNATURE_LEN] = parsed
        .signature
        .as_slice()
        .try_into()
        .expect("parsed ed25519 signature length is checked");

    let public_key = match VerifyingKey::from_bytes(&public_key_bytes) {
        Ok(public_key) => public_key,
        Err(error) => {
            reasons.push(format!("invalid ed25519 public key: {error}"));
            return SignatureVerifyResult {
                artifact,
                signed,
                algorithm: Some(parsed.algorithm),
                public_key_ref: Some(parsed.public_key_ref()),
                public_key_sha256: Some(parsed.public_key_sha256_ref()),
                passed: false,
                reasons,
            };
        }
    };
    let signature = Signature::from_bytes(&signature_bytes);

    match public_key.verify(content, &signature) {
        Ok(()) => SignatureVerifyResult {
            artifact,
            signed,
            algorithm: Some(parsed.algorithm),
            public_key_ref: Some(parsed.public_key_ref()),
            public_key_sha256: Some(parsed.public_key_sha256_ref()),
            passed: true,
            reasons,
        },
        Err(error) => {
            reasons.push(format!("signature verification failed: {error}"));
            SignatureVerifyResult {
                artifact,
                signed,
                algorithm: Some(parsed.algorithm),
                public_key_ref: Some(parsed.public_key_ref()),
                public_key_sha256: Some(parsed.public_key_sha256_ref()),
                passed: false,
                reasons,
            }
        }
    }
}

pub fn verify_signature_file(
    path: impl AsRef<Path>,
    signed: bool,
    signature: Option<&str>,
    allowed_algorithms: &[String],
) -> MspResult<SignatureVerifyResult> {
    let path = path.as_ref();
    let content = std::fs::read(path)?;
    Ok(verify_signature_bytes(
        path.display().to_string(),
        &content,
        signed,
        signature,
        allowed_algorithms,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    #[test]
    fn verifies_ed25519_signature() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let content = b"hello msp";
        let signature = signing_key.sign(content);
        let encoded = ParsedSignature::encode_ed25519(
            signing_key.verifying_key().as_bytes(),
            &signature.to_bytes(),
        )
        .unwrap();

        let result = verify_signature_bytes(
            "artifact",
            content,
            true,
            Some(&encoded),
            &["ed25519".to_string()],
        );
        assert!(result.passed);
        assert_eq!(result.algorithm, Some(SignatureAlgorithm::Ed25519));
    }

    #[test]
    fn exposes_public_key_refs() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let content = b"hello msp";
        let signature = signing_key.sign(content);
        let encoded = ParsedSignature::encode_ed25519(
            signing_key.verifying_key().as_bytes(),
            &signature.to_bytes(),
        )
        .unwrap();
        let parsed = ParsedSignature::parse(&encoded).unwrap();

        assert_eq!(
            parsed.public_key_ref(),
            format!(
                "ed25519:{}",
                STANDARD.encode(signing_key.verifying_key().as_bytes())
            )
        );
        assert!(parsed.public_key_matches_ref(&parsed.public_key_ref()));
        assert!(parsed.public_key_matches_ref(&parsed.public_key_sha256_ref()));
    }

    #[test]
    fn rejects_tampered_content() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let signature = signing_key.sign(b"original");
        let encoded = ParsedSignature::encode_ed25519(
            signing_key.verifying_key().as_bytes(),
            &signature.to_bytes(),
        )
        .unwrap();

        let result = verify_signature_bytes(
            "artifact",
            b"tampered",
            true,
            Some(&encoded),
            &["ed25519".to_string()],
        );
        assert!(!result.passed);
    }

    #[test]
    fn signs_with_raw_ed25519_seed() {
        let seed = [7_u8; 32];
        let content = b"hello msp";
        let encoded = sign_ed25519_bytes(content, &seed).unwrap();
        let result = verify_signature_bytes(
            "artifact",
            content,
            true,
            Some(&encoded),
            &["ed25519".to_string()],
        );
        assert!(result.passed, "{result:?}");
    }

    #[test]
    fn rejects_disallowed_algorithm() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let content = b"hello msp";
        let signature = signing_key.sign(content);
        let encoded = ParsedSignature::encode_ed25519(
            signing_key.verifying_key().as_bytes(),
            &signature.to_bytes(),
        )
        .unwrap();

        let result = verify_signature_bytes("artifact", content, true, Some(&encoded), &[]);
        assert!(result.passed);
        let result = verify_signature_bytes(
            "artifact",
            content,
            true,
            Some(&encoded),
            &["ecdsa-p256".to_string()],
        );
        assert!(!result.passed);
        assert!(result.reasons[0].contains("not allowed"));
    }
}
