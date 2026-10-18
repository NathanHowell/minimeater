use heapless::Vec;

use crate::{
    TlsError,
    buffer::CryptoBuffer,
    parse_buffer::{ParseBuffer, ParseError},
};

#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum NamedGroup {
    /* Elliptic Curve Groups (ECDHE) */
    Secp256r1,
    Secp384r1,
    Secp521r1,
    X25519,
    X448,

    /* Finite Field Groups (DHE) */
    Ffdhe2048,
    Ffdhe3072,
    Ffdhe4096,
    Ffdhe6144,
    Ffdhe8192,

    /* Post-quantum hybrid groups */
    X25519MLKEM768,
    SecP256r1MLKEM768,
    SecP384r1MLKEM1024,
}

impl NamedGroup {
    pub fn parse(buf: &mut ParseBuffer) -> Result<Self, ParseError> {
        Self::from_u16(buf.read_u16()?).ok_or(ParseError::InvalidData)
    }

    /// The group with code point `code`, or `None` for one this crate does not know.
    #[must_use]
    pub fn from_u16(code: u16) -> Option<Self> {
        let known = match code {
            0x0017 => Self::Secp256r1,
            0x0018 => Self::Secp384r1,
            0x0019 => Self::Secp521r1,
            0x001D => Self::X25519,
            0x001E => Self::X448,

            0x0100 => Self::Ffdhe2048,
            0x0101 => Self::Ffdhe3072,
            0x0102 => Self::Ffdhe4096,
            0x0103 => Self::Ffdhe6144,
            0x0104 => Self::Ffdhe8192,

            0x11EB => Self::SecP256r1MLKEM768,
            0x11EC => Self::X25519MLKEM768,
            0x11ED => Self::SecP384r1MLKEM1024,

            _ => return None,
        };
        Some(known)
    }

    pub fn as_u16(self) -> u16 {
        match self {
            Self::Secp256r1 => 0x0017,
            Self::Secp384r1 => 0x0018,
            Self::Secp521r1 => 0x0019,
            Self::X25519 => 0x001D,
            Self::X448 => 0x001E,

            Self::Ffdhe2048 => 0x0100,
            Self::Ffdhe3072 => 0x0101,
            Self::Ffdhe4096 => 0x0102,
            Self::Ffdhe6144 => 0x0103,
            Self::Ffdhe8192 => 0x0104,

            Self::SecP256r1MLKEM768 => 0x11EB,
            Self::X25519MLKEM768 => 0x11EC,
            Self::SecP384r1MLKEM1024 => 0x11ED,
        }
    }

    pub fn encode(self, buf: &mut CryptoBuffer) -> Result<(), TlsError> {
        buf.push_u16(self.as_u16())
            .map_err(|_| TlsError::EncodeError)
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SupportedGroups<const N: usize> {
    pub supported_groups: Vec<NamedGroup, N>,
}

impl<const N: usize> SupportedGroups<N> {
    pub fn parse(buf: &mut ParseBuffer) -> Result<Self, ParseError> {
        let data_length = buf.read_u16()? as usize;
        let mut list = buf.slice(data_length)?;

        // RFC 8446 section 4.2.7 (and 4.2.3): a group this side does not know is skipped, not an error (OpenSSL 4's EncryptedExtensions
        // lists 0x11ee and 0x0029), and a list longer than `N` keeps its first `N`
        let mut supported_groups = Vec::new();
        while !list.is_empty() {
            if let Some(group) = NamedGroup::from_u16(list.read_u16()?) {
                let _ = supported_groups.push(group);
            }
        }

        Ok(Self { supported_groups })
    }

    pub fn encode(&self, buf: &mut CryptoBuffer) -> Result<(), TlsError> {
        buf.with_u16_length(|buf| {
            for g in &self.supported_groups {
                g.encode(buf)?;
            }
            Ok(())
        })
    }
}
