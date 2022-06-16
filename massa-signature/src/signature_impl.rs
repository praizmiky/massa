// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::MassaSignatureError;
use massa_hash::Hash;
use massa_serialization::Deserializer;
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use secp256k1::{schnorr, Message, SECP256K1};
use std::{convert::TryInto, str::FromStr};

/// Size of a private key
pub const PRIVATE_KEY_SIZE_BYTES: usize = 32;
/// Size of a public key
pub const PUBLIC_KEY_SIZE_BYTES: usize = 32;
/// Size of a signature
pub const SIGNATURE_SIZE_BYTES: usize = 64;
const PRIVATE_KEY_STRING_PREFIX: &str = "PRI";
const PUBLIC_KEY_STRING_PREFIX: &str = "PUB";
const SIGNATURE_STRING_PREFIX: &str = "SIG";

/// `PrivateKey` used to sign messages.
/// Schnorr signatures require a [KeyPair](secp256k1::KeyPair) to be signed.
/// The KeyPair is generated when deserializing a private key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateKey(secp256k1::KeyPair);

impl std::fmt::Display for PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(f, "{}-{}", PRIVATE_KEY_STRING_PREFIX, self.to_bs58_check())
        } else {
            write!(f, "{}", self.to_bs58_check())
        }
    }
}

impl FromStr for PrivateKey {
    type Err = MassaSignatureError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if cfg!(feature = "hash-prefix") {
            let v: Vec<_> = s.split('-').collect();
            if v.len() != 2 {
                // assume there is no prefix
                PrivateKey::from_bs58_check(s)
            } else if v[0] != PRIVATE_KEY_STRING_PREFIX {
                Err(MassaSignatureError::WrongPrefix(
                    PRIVATE_KEY_STRING_PREFIX.to_string(),
                    v[0].to_string(),
                ))
            } else {
                PrivateKey::from_bs58_check(v[1])
            }
        } else {
            PrivateKey::from_bs58_check(s)
        }
    }
}

impl PrivateKey {
    /// Serialize a `PrivateKey` using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::generate_random_private_key;
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let serialized: String = private_key.to_bs58_check();
    /// ```
    pub fn to_bs58_check(&self) -> String {
        bs58::encode(self.to_bytes()).with_check().into_string()
    }

    /// Serialize a `PrivateKey` as bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::generate_random_private_key;
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let serialized = private_key.to_bytes();
    /// ```
    pub fn to_bytes(&self) -> [u8; PRIVATE_KEY_SIZE_BYTES] {
        // note: should return a ref to respect conventions
        // but KeyPair has no function to do so
        self.0.secret_bytes()
    }

    /// Serialize a `PrivateKey` into bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::generate_random_private_key;
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let serialized = private_key.into_bytes();
    /// ```
    pub fn into_bytes(self) -> [u8; PRIVATE_KEY_SIZE_BYTES] {
        self.0.secret_bytes()
    }

    /// Deserialize a `PrivateKey` using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{PrivateKey, generate_random_private_key};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let serialized: String = private_key.to_bs58_check();
    /// let deserialized: PrivateKey = PrivateKey::from_bs58_check(&serialized).unwrap();
    /// ```
    pub fn from_bs58_check(data: &str) -> Result<PrivateKey, MassaSignatureError> {
        bs58::decode(data)
            .with_check(None)
            .into_vec()
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!(
                    "private key bs58_check parsing error: {}",
                    err
                ))
            })
            .and_then(|key| {
                PrivateKey::from_bytes(&key.try_into().map_err(|err| {
                    MassaSignatureError::ParsingError(format!(
                        "private key bs58_check parsing error: {:?}",
                        err
                    ))
                })?)
            })
    }

    /// Deserialize a `PrivateKey` from bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{PrivateKey, generate_random_private_key};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let serialized = private_key.to_bytes();
    /// let deserialized: PrivateKey = PrivateKey::from_bytes(&serialized).unwrap();
    /// ```
    pub fn from_bytes(
        data: &[u8; PRIVATE_KEY_SIZE_BYTES],
    ) -> Result<PrivateKey, MassaSignatureError> {
        secp256k1::KeyPair::from_seckey_slice(SECP256K1, &data[..])
            .map(PrivateKey)
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!(
                    "private key bytes parsing error: {}",
                    err
                ))
            })
    }
}

impl ::serde::Serialize for PrivateKey {
    /// `::serde::Serialize` trait for `PrivateKey`
    /// if the serializer is human readable,
    /// serialization is done using `serialize_bs58_check`
    /// else, it uses `serialize_binary`
    ///
    /// # Example
    ///
    /// Human readable serialization :
    /// ```
    /// # use massa_signature::generate_random_private_key;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let serialized: String = serde_json::to_string(&private_key).unwrap();
    /// ```
    ///
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.to_bs58_check())
        } else {
            s.serialize_bytes(&self.to_bytes())
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for PrivateKey {
    /// `::serde::Deserialize` trait for `PrivateKey`
    /// if the deserializer is human readable,
    /// deserialization is done using `deserialize_bs58_check`
    /// else, it uses `deserialize_binary`
    ///
    /// # Example
    ///
    /// Human readable deserialization :
    /// ```
    /// # use massa_signature::{PrivateKey, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let serialized = serde_json::to_string(&private_key).unwrap();
    /// let deserialized: PrivateKey = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<PrivateKey, D::Error> {
        if d.is_human_readable() {
            struct Base58CheckVisitor;

            impl<'de> ::serde::de::Visitor<'de> for Base58CheckVisitor {
                type Value = PrivateKey;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("an ASCII base58check string")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        PrivateKey::from_bs58_check(v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    PrivateKey::from_bs58_check(v).map_err(E::custom)
                }
            }
            d.deserialize_str(Base58CheckVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = PrivateKey;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    PrivateKey::from_bytes(&v.try_into().map_err(E::custom)?).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

/// Public key used to check if a message was encoded
/// by the corresponding `PublicKey`.
/// Generated from the `PrivateKey` using `SignatureEngine`
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PublicKey(secp256k1::XOnlyPublicKey);

impl std::fmt::Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(f, "{}-{}", PUBLIC_KEY_STRING_PREFIX, self.to_bs58_check())
        } else {
            write!(f, "{}", self.to_bs58_check())
        }
    }
}

impl FromStr for PublicKey {
    type Err = MassaSignatureError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if cfg!(feature = "hash-prefix") {
            let v: Vec<_> = s.split('-').collect();
            if v.len() != 2 {
                // assume there is no prefix
                PublicKey::from_bs58_check(s)
            } else if v[0] != PUBLIC_KEY_STRING_PREFIX {
                Err(MassaSignatureError::WrongPrefix(
                    PUBLIC_KEY_STRING_PREFIX.to_string(),
                    v[0].to_string(),
                ))
            } else {
                PublicKey::from_bs58_check(v[1])
            }
        } else {
            PublicKey::from_bs58_check(s)
        }
    }
}

impl PublicKey {
    /// Serialize a `PublicKey` using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{derive_public_key, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    ///
    /// let serialized: String = public_key.to_bs58_check();
    /// ```
    pub fn to_bs58_check(&self) -> String {
        bs58::encode(self.to_bytes()).with_check().into_string()
    }

    /// Serialize a `PublicKey` as bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{derive_public_key, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    ///
    /// let serialize = public_key.to_bytes();
    /// ```
    pub fn to_bytes(&self) -> [u8; PUBLIC_KEY_SIZE_BYTES] {
        self.0.serialize()
    }

    /// Serialize into bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{derive_public_key, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    ///
    /// let serialize = public_key.to_bytes();
    /// ```
    pub fn into_bytes(self) -> [u8; PUBLIC_KEY_SIZE_BYTES] {
        self.0.serialize()
    }

    /// Deserialize a `PublicKey` using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{PublicKey, derive_public_key, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    ///
    /// let serialized: String = public_key.to_bs58_check();
    /// let deserialized: PublicKey = PublicKey::from_bs58_check(&serialized).unwrap();
    /// ```
    pub fn from_bs58_check(data: &str) -> Result<PublicKey, MassaSignatureError> {
        bs58::decode(data)
            .with_check(None)
            .into_vec()
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!(
                    "public key bs58_check parsing error: {}",
                    err
                ))
            })
            .and_then(|key| {
                PublicKey::from_bytes(&key.try_into().map_err(|err| {
                    MassaSignatureError::ParsingError(format!(
                        "public key bs58_check parsing error: {:?}",
                        err
                    ))
                })?)
            })
    }

    /// Deserialize a `PublicKey` from bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{PublicKey, derive_public_key, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    ///
    /// let serialized = public_key.into_bytes();
    /// let deserialized: PublicKey = PublicKey::from_bytes(&serialized).unwrap();
    /// ```
    pub fn from_bytes(
        data: &[u8; PUBLIC_KEY_SIZE_BYTES],
    ) -> Result<PublicKey, MassaSignatureError> {
        secp256k1::XOnlyPublicKey::from_slice(&data[..])
            .map(PublicKey)
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!(
                    "public key bytes parsing error: {}",
                    err
                ))
            })
    }
}

/// Serializer for `Signature`
#[derive(Default)]
pub struct PublicKeyDeserializer;

impl PublicKeyDeserializer {
    /// Creates a `SignatureDeserializer`
    pub fn new() -> Self {
        Self
    }
}

impl Deserializer<PublicKey> for PublicKeyDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], PublicKey, E> {
        let key = PublicKey::from_bytes(buffer.try_into().map_err(|_| {
            nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::LengthValue,
            ))
        })?)
        .map_err(|_| {
            nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::Fail,
            ))
        })?;
        // Safe because the signature deserialization success
        Ok((&buffer[PUBLIC_KEY_SIZE_BYTES..], key))
    }
}

impl ::serde::Serialize for PublicKey {
    /// `::serde::Serialize` trait for `PublicKey`
    /// if the serializer is human readable,
    /// serialization is done using `serialize_bs58_check`
    /// else, it uses `serialize_binary`
    ///
    /// # Example
    ///
    /// Human readable serialization :
    /// ```
    /// # use massa_signature::{derive_public_key, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    ///
    /// let serialized: String = serde_json::to_string(&public_key).unwrap();
    /// ```
    ///
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.to_bs58_check())
        } else {
            s.serialize_bytes(&self.to_bytes())
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for PublicKey {
    /// `::serde::Deserialize` trait for `PublicKey`
    /// if the deserializer is human readable,
    /// deserialization is done using `deserialize_bs58_check`
    /// else, it uses `deserialize_binary`
    ///
    /// # Example
    ///
    /// Human readable deserialization :
    /// ```
    /// # use massa_signature::{PublicKey, derive_public_key, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    ///
    /// let serialized = serde_json::to_string(&public_key).unwrap();
    /// let deserialized: PublicKey = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<PublicKey, D::Error> {
        if d.is_human_readable() {
            struct Base58CheckVisitor;

            impl<'de> ::serde::de::Visitor<'de> for Base58CheckVisitor {
                type Value = PublicKey;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("an ASCII base58check string")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        PublicKey::from_bs58_check(v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    PublicKey::from_bs58_check(v).map_err(E::custom)
                }
            }
            d.deserialize_str(Base58CheckVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = PublicKey;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    PublicKey::from_bytes(v.try_into().map_err(E::custom)?).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

/// Signature generated from a message and a `PrivateKey`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Signature(schnorr::Signature);

impl std::fmt::Display for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(f, "{}-{}", SIGNATURE_STRING_PREFIX, self.to_bs58_check())
        } else {
            write!(f, "{}", self.to_bs58_check())
        }
    }
}

impl FromStr for Signature {
    type Err = MassaSignatureError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if cfg!(feature = "hash-prefix") {
            let v: Vec<_> = s.split('-').collect();
            if v.len() != 2 {
                // assume there is no prefix
                Signature::from_bs58_check(s)
            } else if v[0] != SIGNATURE_STRING_PREFIX {
                Err(MassaSignatureError::WrongPrefix(
                    SIGNATURE_STRING_PREFIX.to_string(),
                    v[0].to_string(),
                ))
            } else {
                Signature::from_bs58_check(v[1])
            }
        } else {
            Signature::from_bs58_check(s)
        }
    }
}

impl Signature {
    /// Serialize a `Signature` using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{generate_random_private_key, sign};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    ///
    /// let serialized: String = signature.to_bs58_check();
    /// ```
    pub fn to_bs58_check(&self) -> String {
        bs58::encode(self.to_bytes()).with_check().into_string()
    }

    /// Serialize a Signature as bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{generate_random_private_key, sign};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    ///
    /// let serialized = signature.to_bytes();
    /// ```
    pub fn to_bytes(&self) -> &[u8; SIGNATURE_SIZE_BYTES] {
        self.0.as_ref()
    }

    /// Serialize a Signature into bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{generate_random_private_key, sign};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    ///
    /// let serialized = signature.into_bytes();
    /// ```
    pub fn into_bytes(self) -> [u8; SIGNATURE_SIZE_BYTES] {
        *self.0.as_ref()
    }

    /// Deserialize a `Signature` using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{generate_random_private_key, sign, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    ///
    /// let serialized: String = signature.to_bs58_check();
    /// let deserialized: Signature = Signature::from_bs58_check(&serialized).unwrap();
    /// ```
    pub fn from_bs58_check(data: &str) -> Result<Signature, MassaSignatureError> {
        bs58::decode(data)
            .with_check(None)
            .into_vec()
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!(
                    "signature bs58_check parsing error: {}",
                    err
                ))
            })
            .and_then(|signature| {
                Signature::from_bytes(&signature.try_into().map_err(|err| {
                    MassaSignatureError::ParsingError(format!(
                        "signature bs58_check parsing error: {:?}",
                        err
                    ))
                })?)
            })
    }

    /// Deserialize a Signature from bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{generate_random_private_key, sign, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    ///
    /// let serialized = signature.to_bytes();
    /// let deserialized: Signature = Signature::from_bytes(&serialized).unwrap();
    /// ```
    pub fn from_bytes(data: &[u8; SIGNATURE_SIZE_BYTES]) -> Result<Signature, MassaSignatureError> {
        schnorr::Signature::from_slice(&data[..])
            .map(Signature)
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!("signature bytes parsing error: {}", err))
            })
    }
}

impl ::serde::Serialize for Signature {
    /// `::serde::Serialize` trait for `Signature`
    /// if the serializer is human readable,
    /// serialization is done using `to_bs58_check`
    /// else, it uses `to_bytes`
    ///
    /// # Example
    ///
    /// Human readable serialization :
    /// ```
    /// # use massa_signature::{generate_random_private_key, sign};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    ///
    /// let serialized: String = serde_json::to_string(&signature).unwrap();
    /// ```
    ///
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.to_bs58_check())
        } else {
            s.serialize_bytes(self.to_bytes())
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for Signature {
    /// `::serde::Deserialize` trait for `Signature`
    /// if the deserializer is human readable,
    /// deserialization is done using `from_bs58_check`
    /// else, it uses `from_bytes`
    ///
    /// # Example
    ///
    /// Human readable deserialization :
    /// ```
    /// # use massa_signature::{generate_random_private_key, sign, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    ///
    /// let serialized = serde_json::to_string(&signature).unwrap();
    /// let deserialized: Signature = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<Signature, D::Error> {
        if d.is_human_readable() {
            struct Base58CheckVisitor;

            impl<'de> ::serde::de::Visitor<'de> for Base58CheckVisitor {
                type Value = Signature;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("an ASCII base58check string")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        Signature::from_bs58_check(v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Signature::from_bs58_check(v).map_err(E::custom)
                }
            }
            d.deserialize_str(Base58CheckVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = Signature;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Signature::from_bytes(v.try_into().map_err(E::custom)?).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

/// Serializer for `Signature`
#[derive(Default)]
pub struct SignatureDeserializer;

impl SignatureDeserializer {
    /// Creates a `SignatureDeserializer`
    pub fn new() -> Self {
        Self
    }
}

impl Deserializer<Signature> for SignatureDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Signature, E> {
        let signature = Signature::from_bytes(buffer.try_into().map_err(|_| {
            nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::LengthValue,
            ))
        })?)
        .map_err(|_| {
            nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::Fail,
            ))
        })?;
        // Safe because the signature deserialization success
        Ok((&buffer[SIGNATURE_SIZE_BYTES..], signature))
    }
}

/// Derives a PublicKey from a PrivateKey.
///
/// # Example
/// ```
/// # use massa_signature::{derive_public_key, generate_random_private_key};
/// # use serde::{Deserialize, Serialize};
/// let private_key = generate_random_private_key();
/// let public_key = derive_public_key(&private_key);
/// ```
pub fn derive_public_key(private_key: &PrivateKey) -> PublicKey {
    PublicKey(private_key.0.public_key())
}

/// Returns the Signature produced by signing
/// data bytes with a PrivateKey.
///
/// # Example
///  ```
/// # use massa_signature::{derive_public_key, generate_random_private_key, sign, PublicKey};
/// # use massa_hash::Hash;
/// # use serde::{Deserialize, Serialize};
/// let private_key = generate_random_private_key();
/// let public_key: PublicKey = derive_public_key(&private_key);
/// let data = Hash::compute_from("Hello World!".as_bytes());
/// let signature = sign(&data, &private_key).unwrap();
/// ```
pub fn sign(hash: &Hash, private_key: &PrivateKey) -> Result<Signature, MassaSignatureError> {
    let message = Message::from_slice(hash.to_bytes())?;
    Ok(Signature(SECP256K1.sign_schnorr(&message, &private_key.0)))
}

/// Checks if the `Signature` associated with data bytes
/// was produced with the `PrivateKey` associated to given `PublicKey`
///
/// # Example
///  ```
/// # use massa_signature::{derive_public_key, generate_random_private_key, sign, verify_signature, PublicKey};
/// # use massa_hash::Hash;
/// # use serde::{Deserialize, Serialize};
/// let private_key = generate_random_private_key();
/// let public_key: PublicKey = derive_public_key(&private_key);
/// let data = Hash::compute_from("Hello World!".as_bytes());
/// let signature = sign(&data, &private_key).unwrap();
/// let verification: bool = verify_signature(&data, &signature, &public_key).is_ok();
/// ```
pub fn verify_signature(
    hash: &Hash,
    signature: &Signature,
    public_key: &PublicKey,
) -> Result<(), MassaSignatureError> {
    let message = Message::from_slice(hash.to_bytes())?;
    Ok(SECP256K1.verify_schnorr(&signature.0, &message, &public_key.0)?)
}

/// Generate a random private key from a RNG.
pub fn generate_random_private_key() -> PrivateKey {
    use secp256k1::rand::rngs::OsRng;
    let mut rng = OsRng::new().expect("OsRng");
    PrivateKey(secp256k1::KeyPair::from_secret_key(
        SECP256K1,
        secp256k1::SecretKey::new(&mut rng),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_hash::Hash;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_example() {
        let private_key = generate_random_private_key();
        let public_key = derive_public_key(&private_key);
        let message = "Hello World!".as_bytes();
        let hash = Hash::compute_from(message);
        let signature = sign(&hash, &private_key).unwrap();
        assert!(verify_signature(&hash, &signature, &public_key).is_ok())
    }

    #[test]
    #[serial]
    fn test_serde_private_key() {
        let private_key = generate_random_private_key();
        let serialized =
            serde_json::to_string(&private_key).expect("could not serialize private key");
        let deserialized =
            serde_json::from_str(&serialized).expect("could not deserialize private key");
        assert_eq!(private_key, deserialized);
    }

    #[test]
    #[serial]
    fn test_serde_public_key() {
        let private_key = generate_random_private_key();
        let public_key = derive_public_key(&private_key);
        let serialized =
            serde_json::to_string(&public_key).expect("Could not serialize public key");
        let deserialized =
            serde_json::from_str(&serialized).expect("could not deserialize public key");
        assert_eq!(public_key, deserialized);
    }

    #[test]
    #[serial]
    fn test_serde_signature() {
        let private_key = generate_random_private_key();
        let message = "Hello World!".as_bytes();
        let hash = Hash::compute_from(message);
        let signature = sign(&hash, &private_key).unwrap();
        let serialized =
            serde_json::to_string(&signature).expect("could not serialize signature key");
        let deserialized =
            serde_json::from_str(&serialized).expect("could not deserialize signature key");
        assert_eq!(signature, deserialized);
    }
}