// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the structure representing an entry in the `FinalLedger`

use crate::ledger_changes::LedgerEntryUpdate;
use crate::types::{Applicable, SetOrDelete};
use massa_models::amount::{Amount, AmountDeserializer, AmountSerializer};
use massa_models::datastore::{Datastore, DatastoreDeserializer, DatastoreSerializer};
use massa_models::serialization::{VecU8Deserializer, VecU8Serializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::error::{context, ContextError, ParseError};
use nom::multi::length_count;
use nom::sequence::tuple;
use nom::{IResult, Parser};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Bound::Included;

/// Structure defining an entry associated to an address in the `FinalLedger`
#[derive(Default, Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct LedgerEntry {
    /// The parallel balance of that entry.
    /// See lib.rs for an explanation on sequential vs parallel balance.
    pub sequential_balance: Amount,

    /// The parallel balance of that entry.
    pub parallel_balance: Amount,

    /// Executable bytecode
    pub bytecode: Vec<u8>,

    /// A key-value store associating a hash to arbitrary bytes
    pub datastore: Datastore,
}

/// Serializer for `LedgerEntry`
pub struct LedgerEntrySerializer {
    amount_serializer: AmountSerializer,
    vec_u8_serializer: VecU8Serializer,
    datastore_serializer: DatastoreSerializer,
}

impl LedgerEntrySerializer {
    /// Creates a new `LedgerEntrySerializer`
    pub fn new() -> Self {
        Self {
            vec_u8_serializer: VecU8Serializer::new(),
            amount_serializer: AmountSerializer::new(),
            datastore_serializer: DatastoreSerializer::new(),
        }
    }
}

impl Default for LedgerEntrySerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<LedgerEntry> for LedgerEntrySerializer {
    /// ## Example
    /// ```
    /// use massa_serialization::Serializer;
    /// use std::collections::BTreeMap;
    /// use std::str::FromStr;
    /// use massa_models::amount::Amount;
    /// use massa_ledger_exports::{LedgerEntry, LedgerEntrySerializer};
    ///
    /// let key = "hello world".as_bytes().to_vec();
    /// let mut store = BTreeMap::new();
    /// store.insert(key, vec![1, 2, 3]);
    /// let amount = Amount::from_str("1").unwrap();
    /// let bytecode = vec![1, 2, 3];
    /// let ledger_entry = LedgerEntry {
    ///    parallel_balance: amount,
    ///    sequential_balance: amount,
    ///    bytecode,
    ///    datastore: store,
    /// };
    /// let mut serialized = Vec::new();
    /// let serializer = LedgerEntrySerializer::new();
    /// serializer.serialize(&ledger_entry, &mut serialized).unwrap();
    /// ```
    fn serialize(&self, value: &LedgerEntry, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.amount_serializer
            .serialize(&value.sequential_balance, buffer)?;
        self.amount_serializer
            .serialize(&value.parallel_balance, buffer)?;
        self.vec_u8_serializer.serialize(&value.bytecode, buffer)?;
        self.datastore_serializer
            .serialize(&value.datastore, buffer)?;
        Ok(())
    }
}

/// Deserializer for `LedgerEntry`
pub struct LedgerEntryDeserializer {
    amount_deserializer: AmountDeserializer,
    bytecode_deserializer: VecU8Deserializer,
    datastore_deserializer: DatastoreDeserializer,
}

impl LedgerEntryDeserializer {
    /// Creates a new `LedgerEntryDeserializer`
    pub fn new(
        max_datastore_entry_count: u64,
        max_datastore_key_length: u8,
        max_datastore_value_length: u64,
    ) -> Self {
        Self {
            amount_deserializer: AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            ),
            bytecode_deserializer: VecU8Deserializer::new(
                Included(u64::MIN),
                Included(max_datastore_value_length),
            ),
            datastore_deserializer: DatastoreDeserializer::new(
                max_datastore_entry_count,
                max_datastore_key_length,
                max_datastore_value_length,
            ),
        }
    }
}

impl Deserializer<LedgerEntry> for LedgerEntryDeserializer {
    /// ## Example
    /// ```
    /// use massa_serialization::{Deserializer, Serializer, DeserializeError};
    /// use std::collections::BTreeMap;
    /// use std::str::FromStr;
    /// use massa_models::amount::Amount;
    /// use massa_ledger_exports::{LedgerEntry, LedgerEntrySerializer, LedgerEntryDeserializer};
    ///
    /// let key = "hello world".as_bytes().to_vec();
    /// let mut store = BTreeMap::new();
    /// store.insert(key, vec![1, 2, 3]);
    /// let amount = Amount::from_str("1").unwrap();
    /// let bytecode = vec![1, 2, 3];
    /// let ledger_entry = LedgerEntry {
    ///    parallel_balance: amount,
    ///    sequential_balance: amount,
    ///    bytecode,
    ///    datastore: store,
    /// };
    /// let mut serialized = Vec::new();
    /// let serializer = LedgerEntrySerializer::new();
    /// let deserializer = LedgerEntryDeserializer::new(10000, 255, 10000);
    /// serializer.serialize(&ledger_entry, &mut serialized).unwrap();
    /// let (rest, ledger_entry_deser) = deserializer.deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(ledger_entry, ledger_entry_deser);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], LedgerEntry, E> {
        context(
            "Failed LedgerEntry deserialization",
            tuple((
                context("Failed sequential_balance deserialization", |input| {
                    self.amount_deserializer.deserialize(input)
                }),
                context("Failed parallel_balance deserialization", |input| {
                    self.amount_deserializer.deserialize(input)
                }),
                context("Failed bytecode deserialization", |input| {
                    self.bytecode_deserializer.deserialize(input)
                }),
                context("Failed datastore deserialization", |input| {
                    self.datastore_deserializer.deserialize(input)
                }),
            )),
        )
        .map(
            |(sequential_balance, parallel_balance, bytecode, datastore)| LedgerEntry {
                sequential_balance,
                parallel_balance,
                bytecode,
                datastore,
            },
        )
        .parse(buffer)
    }
}

/// A `LedgerEntryUpdate` can be applied to a `LedgerEntry`
impl Applicable<LedgerEntryUpdate> for LedgerEntry {
    fn apply(&mut self, update: LedgerEntryUpdate) {
        // apply updates to the parallel balance
        update.parallel_balance.apply_to(&mut self.parallel_balance);

        // apply updates to the executable bytecode
        update.bytecode.apply_to(&mut self.bytecode);

        // iterate over all datastore updates
        for (key, value_update) in update.datastore {
            match value_update {
                // this update sets a new value to a datastore entry
                SetOrDelete::Set(v) => {
                    // insert the new value in the datastore,
                    // replacing any existing value
                    self.datastore.insert(key, v);
                }

                // this update deletes a datastore entry
                SetOrDelete::Delete => {
                    // remove that entry from the datastore if it exists
                    self.datastore.remove(&key);
                }
            }
        }
    }
}
