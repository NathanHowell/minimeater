use crate::{
    TlsError,
    buffer::CryptoBuffer,
    parse_buffer::{ParseBuffer, ParseError},
};

use heapless::Vec;

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SignatureScheme {
    /* RSASSA-PKCS1-v1_5 algorithms */
    RsaPkcs1Sha256,
    RsaPkcs1Sha384,
    RsaPkcs1Sha512,

    /* ECDSA algorithms */
    EcdsaSecp256r1Sha256,
    EcdsaSecp384r1Sha384,
    EcdsaSecp521r1Sha512,

    /* RSASSA-PSS algorithms with public key OID rsaEncryption */
    RsaPssRsaeSha256,
    RsaPssRsaeSha384,
    RsaPssRsaeSha512,

    /* EdDSA algorithms */
    Ed25519,
    Ed448,

    /* RSASSA-PSS algorithms with public key OID RSASSA-PSS */
    RsaPssPssSha256,
    RsaPssPssSha384,
    RsaPssPssSha512,

    Sha224Ecdsa,
    Sha224Rsa,
    Sha224Dsa,

    /* Legacy algorithms */
    RsaPkcs1Sha1,
    EcdsaSha1,

    /* Brainpool */
    Sha256BrainpoolP256r1,
    Sha384BrainpoolP384r1,
    Sha512BrainpoolP512r1,

    /* ML-DSA */
    MlDsa44,
    MlDsa65,
    MlDsa87,
    /* Reserved Code Points */
    //private_use(0xFE00..0xFFFF),
    //(0xFFFF)
}

impl SignatureScheme {
    pub fn parse(buf: &mut ParseBuffer) -> Result<Self, ParseError> {
        Self::from_u16(buf.read_u16()?).ok_or(ParseError::InvalidData)
    }

    /// The scheme with code point `code`, or `None` for one this crate does not know.
    #[must_use]
    pub fn from_u16(code: u16) -> Option<Self> {
        let known = match code {
            0x0401 => Self::RsaPkcs1Sha256,
            0x0501 => Self::RsaPkcs1Sha384,
            0x0601 => Self::RsaPkcs1Sha512,

            0x0403 => Self::EcdsaSecp256r1Sha256,
            0x0503 => Self::EcdsaSecp384r1Sha384,
            0x0603 => Self::EcdsaSecp521r1Sha512,

            0x0804 => Self::RsaPssRsaeSha256,
            0x0805 => Self::RsaPssRsaeSha384,
            0x0806 => Self::RsaPssRsaeSha512,

            0x0807 => Self::Ed25519,
            0x0808 => Self::Ed448,

            0x0809 => Self::RsaPssPssSha256,
            0x080a => Self::RsaPssPssSha384,
            0x080b => Self::RsaPssPssSha512,

            0x0303 => Self::Sha224Ecdsa,
            0x0301 => Self::Sha224Rsa,
            0x0302 => Self::Sha224Dsa,

            0x0201 => Self::RsaPkcs1Sha1,
            0x0203 => Self::EcdsaSha1,

            0x081A => Self::Sha256BrainpoolP256r1,
            0x081B => Self::Sha384BrainpoolP384r1,
            0x081C => Self::Sha512BrainpoolP512r1,

            0x0904 => Self::MlDsa44,
            0x0905 => Self::MlDsa65,
            0x0906 => Self::MlDsa87,

            _ => return None,
        };
        Some(known)
    }

    #[must_use]
    pub fn as_u16(self) -> u16 {
        match self {
            Self::RsaPkcs1Sha256 => 0x0401,
            Self::RsaPkcs1Sha384 => 0x0501,
            Self::RsaPkcs1Sha512 => 0x0601,

            Self::EcdsaSecp256r1Sha256 => 0x0403,
            Self::EcdsaSecp384r1Sha384 => 0x0503,
            Self::EcdsaSecp521r1Sha512 => 0x0603,

            Self::RsaPssRsaeSha256 => 0x0804,
            Self::RsaPssRsaeSha384 => 0x0805,
            Self::RsaPssRsaeSha512 => 0x0806,

            Self::Ed25519 => 0x0807,
            Self::Ed448 => 0x0808,

            Self::RsaPssPssSha256 => 0x0809,
            Self::RsaPssPssSha384 => 0x080a,
            Self::RsaPssPssSha512 => 0x080b,

            Self::Sha224Ecdsa => 0x0303,
            Self::Sha224Rsa => 0x0301,
            Self::Sha224Dsa => 0x0302,

            Self::RsaPkcs1Sha1 => 0x0201,
            Self::EcdsaSha1 => 0x0203,

            Self::Sha256BrainpoolP256r1 => 0x081A,
            Self::Sha384BrainpoolP384r1 => 0x081B,
            Self::Sha512BrainpoolP512r1 => 0x081C,

            Self::MlDsa44 => 0x0904,
            Self::MlDsa65 => 0x0905,
            Self::MlDsa87 => 0x0906,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SignatureAlgorithms<const N: usize> {
    pub supported_signature_algorithms: Vec<SignatureScheme, N>,
}

impl<const N: usize> SignatureAlgorithms<N> {
    pub fn parse(buf: &mut ParseBuffer) -> Result<Self, ParseError> {
        let data_length = buf.read_u16()? as usize;
        let mut list = buf.slice(data_length)?;

        // RFC 8446 section 4.2.3: a code point this side does not know is skipped, not an error (OpenSSL 4 lists ones this crate has no
        // name for, 0x0708 among them), and a list longer than `N` keeps its first `N`
        let mut supported_signature_algorithms = Vec::new();
        while !list.is_empty() {
            if let Some(scheme) = SignatureScheme::from_u16(list.read_u16()?) {
                let _ = supported_signature_algorithms.push(scheme);
            }
        }

        Ok(Self {
            supported_signature_algorithms,
        })
    }

    pub fn encode(&self, buf: &mut CryptoBuffer) -> Result<(), TlsError> {
        buf.with_u16_length(|buf| {
            for &a in &self.supported_signature_algorithms {
                buf.push_u16(a.as_u16())
                    .map_err(|_| TlsError::EncodeError)?;
            }
            Ok(())
        })
    }
}
