use crate::LUT_SIGNER_SEED;
use crate::SWITCHBOARD_ON_DEMAND_PROGRAM_ID;
use anyhow_ext::anyhow;
use anyhow_ext::Error as AnyhowError;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::account::Account;
use crate::address_lookup_table::derive_lookup_table_address;
use crate::address_lookup_table::AddressLookupTable;
use solana_sdk::address_lookup_table_account::AddressLookupTableAccount;
use solana_sdk::pubkey::Pubkey;

pub fn find_lut_signer(k: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[LUT_SIGNER_SEED, k.as_ref()],
        &SWITCHBOARD_ON_DEMAND_PROGRAM_ID,
    )
    .0
}

pub trait LutOwner {
    fn lut_slot(&self) -> u64;
}

pub async fn load_lookup_table<T: LutOwner + bytemuck::Pod>(
    client: &RpcClient,
    self_key: Pubkey,
) -> Result<AddressLookupTableAccount, AnyhowError> {
    let account = client
        .get_account_data(&self_key)
        .await
        .map_err(|_| anyhow!("LutOwner.load_lookup_table: Oracle not found"))?;
    let account = account[8..].to_vec();
    let data = bytemuck::try_from_bytes::<T>(&account)
        .map_err(|_| anyhow!("LutOwner.load_lookup_table: Invalid data"))?;
    let lut_slot = data.lut_slot();
    let lut_signer = find_lut_signer(&self_key);
    let lut_key = derive_lookup_table_address(&lut_signer, lut_slot).0;
    let lut_account = client
        .get_account_data(&lut_key)
        .await
        .map_err(|_| anyhow!("LutOwner.load_lookup_table: LUT not found"))?;
    let parsed_lut = AddressLookupTable::deserialize(&lut_account)
        .map_err(|_| anyhow!("LutOwner.load_lookup_table: Invalid LUT data"))?;
    Ok(AddressLookupTableAccount {
        addresses: parsed_lut.addresses.to_vec(),
        key: lut_key,
    })
}

fn account_to_vec(account: Option<Account>) -> Vec<u8> {
    match account {
        Some(account) => account.data.get(8..).unwrap_or(&[]).to_vec(),
        None => vec![],
    }
}

pub async fn load_lookup_tables<T: LutOwner + bytemuck::Pod>(
    client: &RpcClient,
    keys: &[Pubkey],
) -> Result<Vec<AddressLookupTableAccount>, AnyhowError> {
    let accounts_data = client
        .get_multiple_accounts(&keys)
        .await?
        .into_iter()
        .map(account_to_vec)
        .collect::<Vec<_>>();
    let mut lut_keys = Vec::new();
    let mut out = Vec::new();
    for (idx, account) in accounts_data.iter().enumerate() {
        let data = bytemuck::try_from_bytes::<T>(&account)
            .map_err(|_| anyhow!("LutOwner.load_lookup_tables: Invalid data"))?;
        let lut_slot = data.lut_slot();
        let lut_signer = find_lut_signer(&keys[idx]);
        let lut_key = derive_lookup_table_address(&lut_signer, lut_slot).0;
        lut_keys.push(lut_key);
    }
    let lut_datas = client
        .get_multiple_accounts(&lut_keys)
        .await?
        .into_iter()
        .map(|data| data.unwrap_or_default().data.to_vec())
        .collect::<Vec<Vec<u8>>>();
    for (idx, lut_data) in lut_datas.iter().enumerate() {
        let parsed_lut = AddressLookupTable::deserialize(&lut_data)
            .map_err(|_| anyhow!("LutOwner.load_lookup_tables: Invalid LUT data"))?;
        out.push(AddressLookupTableAccount {
            addresses: parsed_lut.addresses.to_vec(),
            key: lut_keys[idx],
        });
    }
    Ok(out)
}
