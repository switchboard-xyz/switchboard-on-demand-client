use serde::Serialize;
use serde::Deserialize;
use solana_sdk::instruction::InstructionError;
use solana_sdk::slot_history::Slot;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use std::borrow::Cow;

/// Derives the address of an address table account from a wallet address and a recent block's slot.
pub fn derive_lookup_table_address(
    authority_address: &Pubkey,
    recent_block_slot: Slot,
) -> (Pubkey, u8) {
    let id = Pubkey::from_str("AddressLookupTab1e1111111111111111111111111").unwrap();

    Pubkey::find_program_address(
        &[authority_address.as_ref(), &recent_block_slot.to_le_bytes()],
        &id
    )
}


#[derive(Debug, PartialEq, Eq, Clone)]
pub struct AddressLookupTable<'a> {
    pub meta: LookupTableMeta,
    pub addresses: Cow<'a, [Pubkey]>,
}

impl<'a> AddressLookupTable<'a> {
    /// Efficiently deserialize an address table without allocating
    /// for stored addresses.
    pub fn deserialize(data: &'a [u8]) -> Result<AddressLookupTable<'a>, InstructionError> {
        let program_state: ProgramState =
            bincode::deserialize(data).map_err(|_| InstructionError::InvalidAccountData)?;

        let meta = match program_state {
            ProgramState::LookupTable(meta) => Ok(meta),
            ProgramState::Uninitialized => Err(InstructionError::UninitializedAccount),
        }?;

        let raw_addresses_data = data.get(LOOKUP_TABLE_META_SIZE..).ok_or({
            // Should be impossible because table accounts must
            // always be LOOKUP_TABLE_META_SIZE in length
            InstructionError::InvalidAccountData
        })?;
        let addresses: &[Pubkey] = bytemuck::try_cast_slice(raw_addresses_data).map_err(|_| {
            // Should be impossible because raw address data
            // should be aligned and sized in multiples of 32 bytes
            InstructionError::InvalidAccountData
        })?;

        Ok(Self {
            meta,
            addresses: Cow::Borrowed(addresses),
        })
    }
}

/// Program account states
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ProgramState {
    /// Account is not initialized.
    Uninitialized,
    /// Initialized `LookupTable` account.
    LookupTable(LookupTableMeta),
}

/// The maximum number of addresses that a lookup table can hold
pub const LOOKUP_TABLE_MAX_ADDRESSES: usize = 256;

/// The serialized size of lookup table metadata
pub const LOOKUP_TABLE_META_SIZE: usize = 56;

/// Activation status of a lookup table
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum LookupTableStatus {
    Activated,
    Deactivating { remaining_blocks: usize },
    Deactivated,
}

/// Address lookup table metadata
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct LookupTableMeta {
    /// Lookup tables cannot be closed until the deactivation slot is
    /// no longer "recent" (not accessible in the `SlotHashes` sysvar).
    pub deactivation_slot: Slot,
    /// The slot that the table was last extended. Address tables may
    /// only be used to lookup addresses that were extended before
    /// the current bank's slot.
    pub last_extended_slot: Slot,
    /// The start index where the table was last extended from during
    /// the `last_extended_slot`.
    pub last_extended_slot_start_index: u8,
    /// Authority address which must sign for each modification.
    pub authority: Option<Pubkey>,
    // Padding to keep addresses 8-byte aligned
    pub _padding: u16,
    // Raw list of addresses follows this serialized structure in
    // the account's data, starting from `LOOKUP_TABLE_META_SIZE`.
}

