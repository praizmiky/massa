// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{
    address::{AddressHashMap, AddressHashSet},
    array_from_slice,
    hhasher::{HHashMap, HHashSet, PreHashed},
    u8_from_slice, with_serialization_context, Address, DeserializeCompact, DeserializeMinBEInt,
    DeserializeVarInt, Endorsement, ModelsError, Operation, OperationHashSet, SerializeCompact,
    SerializeMinBEInt, SerializeVarInt, Slot, SLOT_KEY_SIZE,
};
use crypto::{
    hash::{Hash, HASH_SIZE_BYTES},
    signature::{
        sign, verify_signature, PrivateKey, PublicKey, Signature, PUBLIC_KEY_SIZE_BYTES,
        SIGNATURE_SIZE_BYTES,
    },
};
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::str::FromStr;

pub const BLOCK_ID_SIZE_BYTES: usize = HASH_SIZE_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct BlockId(pub Hash);

impl PreHashed for BlockId {}

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0.to_bs58_check())
    }
}

impl FromStr for BlockId {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(BlockId(Hash::from_str(s)?))
    }
}

impl BlockId {
    pub fn to_bytes(&self) -> [u8; BLOCK_ID_SIZE_BYTES] {
        self.0.to_bytes()
    }

    pub fn into_bytes(self) -> [u8; BLOCK_ID_SIZE_BYTES] {
        self.0.into_bytes()
    }

    pub fn from_bytes(data: &[u8; BLOCK_ID_SIZE_BYTES]) -> Result<BlockId, ModelsError> {
        Ok(BlockId(
            Hash::from_bytes(data).map_err(|_| ModelsError::HashError)?,
        ))
    }
    pub fn from_bs58_check(data: &str) -> Result<BlockId, ModelsError> {
        Ok(BlockId(
            Hash::from_bs58_check(data).map_err(|_| ModelsError::HashError)?,
        ))
    }

    pub fn get_first_bit(&self) -> bool {
        Hash::hash(&self.to_bytes()).to_bytes()[0] >> 7 == 1
    }
}

pub type BlockHashMap<T> = HHashMap<BlockId, T>;
pub type BlockHashSet = HHashSet<BlockId>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub operations: Vec<Operation>,
}

impl Block {
    pub fn contains_operation(&self, op: &Operation) -> Result<bool, ModelsError> {
        let op_id = op.get_operation_id()?;
        Ok(self
            .operations
            .iter()
            .any(|o| o.get_operation_id().map(|id| id == op_id).unwrap_or(false)))
    }

    pub fn bytes_count(&self) -> Result<u64, ModelsError> {
        Ok(self.to_bytes_compact()?.len() as u64)
    }

    /// Retrieve roll involving addresses
    pub fn get_roll_involved_addresses(&self) -> Result<AddressHashSet, ModelsError> {
        let mut roll_involved_addrs = AddressHashSet::default();
        for op in self.operations.iter() {
            roll_involved_addrs.extend(op.get_roll_involved_addresses()?);
        }
        Ok(roll_involved_addrs)
    }

    pub fn involved_addresses(&self) -> Result<AddressHashMap<OperationHashSet>, ModelsError> {
        let mut addresses_to_operations: AddressHashMap<OperationHashSet> =
            AddressHashMap::default();
        self.operations
            .iter()
            .try_for_each::<_, Result<(), ModelsError>>(|op| {
                let addrs = op
                    .get_ledger_involved_addresses(Some(Address::from_public_key(
                        &self.header.content.creator,
                    )?))
                    .map_err(|err| {
                        ModelsError::DeserializeError(format!(
                            "could not get involved addresses: {:?}",
                            err
                        ))
                    })?;
                let id = op.get_operation_id()?;
                for ad in addrs.iter() {
                    if let Some(entry) = addresses_to_operations.get_mut(ad) {
                        entry.insert(id);
                    } else {
                        let mut set = OperationHashSet::default();
                        set.insert(id);
                        addresses_to_operations.insert(*ad, set);
                    }
                }
                Ok(())
            })?;
        Ok(addresses_to_operations)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeaderContent {
    pub creator: PublicKey,
    pub slot: Slot,
    pub parents: Vec<BlockId>,
    pub operation_merkle_root: Hash, // all operations hash
    pub endorsements: Vec<Endorsement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub content: BlockHeaderContent,
    pub signature: Signature,
}

/// Checks performed:
/// - Validity of header.
/// - Number of operations.
/// - Validity of operations.
impl SerializeCompact for Block {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // header
        res.extend(self.header.to_bytes_compact()?);

        let max_block_operations =
            with_serialization_context(|context| context.max_block_operations);

        // operations
        let operation_count: u32 = self.operations.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many operations: {:?}", err))
        })?;
        res.extend(operation_count.to_be_bytes_min(max_block_operations)?);
        for operation in self.operations.iter() {
            res.extend(operation.to_bytes_compact()?);
        }

        Ok(res)
    }
}

/// Checks performed:
/// - Validity of header.
/// - Size of block.
/// - Operation count.
/// - Validity of operation.
impl DeserializeCompact for Block {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        let (max_block_size, max_block_operations) = with_serialization_context(|context| {
            (context.max_block_size, context.max_block_operations)
        });

        // header
        let (header, delta) = BlockHeader::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;
        if cursor > (max_block_size as usize) {
            return Err(ModelsError::DeserializeError("block is too large".into()));
        }

        // operations
        let (operation_count, delta) =
            u32::from_be_bytes_min(&buffer[cursor..], max_block_operations)?;
        cursor += delta;
        if cursor > (max_block_size as usize) {
            return Err(ModelsError::DeserializeError("block is too large".into()));
        }
        let mut operations: Vec<Operation> = Vec::with_capacity(operation_count as usize);
        for _ in 0..(operation_count as usize) {
            let (operation, delta) = Operation::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;
            if cursor > (max_block_size as usize) {
                return Err(ModelsError::DeserializeError("block is too large".into()));
            }
            operations.push(operation);
        }

        Ok((Block { header, operations }, cursor))
    }
}

impl BlockHeader {
    /// Verify the signature of the header
    pub fn check_signature(&self) -> Result<(), ModelsError> {
        let hash = self.content.compute_hash()?;
        self.verify_signature(&hash)?;
        Ok(())
    }

    /// Generate the block id without verifying the integrity of the it,
    /// used only in tests.
    pub fn compute_block_id(&self) -> Result<BlockId, ModelsError> {
        Ok(BlockId(Hash::hash(&self.to_bytes_compact()?)))
    }

    // Hash([slot, hash])
    fn get_signature_message(slot: &Slot, hash: &Hash) -> Hash {
        let mut res = [0u8; SLOT_KEY_SIZE + BLOCK_ID_SIZE_BYTES];
        res[..SLOT_KEY_SIZE].copy_from_slice(&slot.to_bytes_key());
        res[SLOT_KEY_SIZE..].copy_from_slice(&hash.to_bytes());
        // rehash for safety
        Hash::hash(&res)
    }

    // check if a [slot, hash] pair was signed by a public_key
    pub fn verify_slot_hash_signature(
        slot: &Slot,
        hash: &Hash,
        signature: &Signature,
        public_key: &PublicKey,
    ) -> Result<(), ModelsError> {
        verify_signature(
            &BlockHeader::get_signature_message(slot, hash),
            signature,
            public_key,
        )
        .map_err(|err| err.into())
    }

    pub fn new_signed(
        private_key: &PrivateKey,
        content: BlockHeaderContent,
    ) -> Result<(BlockId, Self), ModelsError> {
        let hash = content.compute_hash()?;
        let signature = sign(
            &BlockHeader::get_signature_message(&content.slot, &hash),
            private_key,
        )?;
        let header = BlockHeader { content, signature };
        let block_id = header.compute_block_id()?;
        Ok((block_id, header))
    }

    pub fn verify_signature(&self, hash: &Hash) -> Result<(), ModelsError> {
        BlockHeader::verify_slot_hash_signature(
            &self.content.slot,
            hash,
            &self.signature,
            &self.content.creator,
        )
    }
}

/// Checks performed:
/// - Content.
impl SerializeCompact for BlockHeader {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // signed content
        res.extend(self.content.to_bytes_compact()?);

        // signature
        res.extend(&self.signature.to_bytes());

        Ok(res)
    }
}

/// Checks performed:
/// - Content
/// - Signature.
impl DeserializeCompact for BlockHeader {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        // signed content
        let (content, delta) = BlockHeaderContent::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // signature
        let signature = Signature::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += SIGNATURE_SIZE_BYTES;

        Ok((BlockHeader { content, signature }, cursor))
    }
}

impl BlockHeaderContent {
    pub fn compute_hash(&self) -> Result<Hash, ModelsError> {
        Ok(Hash::hash(&self.to_bytes_compact()?))
    }
}

/// Checks performed:
/// - Validity of slot.
/// - Valid length of included endorsements.
/// - Validity of included endorsements.
impl SerializeCompact for BlockHeaderContent {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // creator public key
        res.extend(&self.creator.to_bytes());

        // slot
        res.extend(self.slot.to_bytes_compact()?);

        // parents (note: there should be none if slot period=0)
        if self.parents.is_empty() {
            res.push(0);
        } else {
            res.push(1);
        }
        for parent_h in self.parents.iter() {
            res.extend(&parent_h.0.to_bytes());
        }

        // operations merkle root
        res.extend(&self.operation_merkle_root.to_bytes());

        // endorsements
        let endorsements_count: u32 = self.endorsements.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many endorsements: {:?}", err))
        })?;
        res.extend(endorsements_count.to_varint_bytes());
        for endorsement in self.endorsements.iter() {
            res.extend(endorsement.to_bytes_compact()?);
        }

        Ok(res)
    }
}

/// Checks performed:
/// - Validity of slot.
/// - Presence of parent.
/// - Valid length of included endorsements.
/// - Validity of included endorsements.
impl DeserializeCompact for BlockHeaderContent {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        // creator public key
        let creator = PublicKey::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += PUBLIC_KEY_SIZE_BYTES;

        // slot
        let (slot, delta) = Slot::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // parents
        let has_parents = u8_from_slice(&buffer[cursor..])?;
        cursor += 1;
        let parent_count = with_serialization_context(|context| context.parent_count);
        let parents = if has_parents == 1 {
            let mut parents: Vec<BlockId> = Vec::with_capacity(parent_count as usize);
            for _ in 0..parent_count {
                let parent_id = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += BLOCK_ID_SIZE_BYTES;
                parents.push(parent_id);
            }
            parents
        } else if has_parents == 0 {
            Vec::new()
        } else {
            return Err(ModelsError::SerializeError(
                "BlockHeaderContent from_bytes_compact bad has parents flags.".into(),
            ));
        };

        // operation merkle tree root
        let operation_merkle_root = Hash::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += HASH_SIZE_BYTES;

        let max_block_endorsments =
            with_serialization_context(|context| context.max_block_endorsments);

        // endorsements
        let (endorsement_count, delta) =
            u32::from_varint_bytes_bounded(&buffer[cursor..], max_block_endorsments)?;
        cursor += delta;

        let mut endorsements: Vec<Endorsement> = Vec::with_capacity(endorsement_count as usize);
        for _ in 0..endorsement_count {
            let (endorsement, delta) = Endorsement::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;
            endorsements.push(endorsement);
        }

        Ok((
            BlockHeaderContent {
                creator,
                slot,
                parents,
                operation_merkle_root,
                endorsements,
            },
            cursor,
        ))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::EndorsementContent;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_block_serialization() {
        let ctx = crate::SerializationContext {
            max_block_size: 1024 * 1024,
            max_block_operations: 1024,
            parent_count: 3,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
            max_bootstrap_pos_cycles: 1000,
            max_bootstrap_pos_entries: 1000,
            max_ask_blocks_per_message: 10,
            max_operations_per_message: 1024,
            max_endorsements_per_message: 1024,
            max_bootstrap_message_size: 100000000,
            max_block_endorsments: 8,
        };
        crate::init_serialization_context(ctx);
        let private_key = crypto::generate_random_private_key();
        let public_key = crypto::derive_public_key(&private_key);

        // create block header
        let (orig_id, orig_header) = BlockHeader::new_signed(
            &private_key,
            BlockHeaderContent {
                creator: public_key,
                slot: Slot::new(1, 2),
                parents: vec![
                    BlockId(Hash::hash("abc".as_bytes())),
                    BlockId(Hash::hash("def".as_bytes())),
                    BlockId(Hash::hash("ghi".as_bytes())),
                ],
                operation_merkle_root: Hash::hash("mno".as_bytes()),
                endorsements: vec![
                    Endorsement {
                        content: EndorsementContent {
                            sender_public_key: public_key,
                            slot: Slot::new(1, 1),
                            index: 1,
                            endorsed_block: BlockId(Hash::hash("blk1".as_bytes())),
                        },
                        signature: sign(&Hash::hash("dta".as_bytes()), &private_key).unwrap(),
                    },
                    Endorsement {
                        content: EndorsementContent {
                            sender_public_key: public_key,
                            slot: Slot::new(4, 0),
                            index: 3,
                            endorsed_block: BlockId(Hash::hash("blk2".as_bytes())),
                        },
                        signature: sign(&Hash::hash("dat".as_bytes()), &private_key).unwrap(),
                    },
                ],
            },
        )
        .unwrap();

        // create block
        let orig_block = Block {
            header: orig_header,
            operations: vec![],
        };

        // serialize block
        let orig_bytes = orig_block.to_bytes_compact().unwrap();

        // deserialize
        let (res_block, res_size) = Block::from_bytes_compact(&orig_bytes).unwrap();
        assert_eq!(orig_bytes.len(), res_size);

        // check equality
        let res_id = res_block.header.compute_block_id().unwrap();
        let generated_res_id = res_block.header.compute_block_id().unwrap();
        assert_eq!(orig_id, res_id);
        assert_eq!(orig_id, generated_res_id);
        assert_eq!(res_block.header.signature, orig_block.header.signature);
    }
}
