use std::array;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, StdResult, Storage, Uint128};
use cw20_ics20_msg::amount::Amount;
use cw20_ics20_msg::converter::ConverterController;
use cw20_ics20_msg::state::{
    AllowInfo, ChannelInfo, ConvertReplyArgs, MappingMetadata, Ratio, ReplyArgs,
};
use cw_controllers::Admin;
use cw_storage_plus::{Index, IndexList, IndexedMap, Item, Map, MultiIndex};
use oraiswap::router::RouterController;

use crate::ContractError;

pub const ADMIN: Admin = Admin::new("admin");

pub const CONFIG: Item<Config> = Item::new("ics20_config_v1.0.2");

// Used to pass info from the ibc_packet_receive to the reply handler
pub const REPLY_ARGS: Item<ReplyArgs> = Item::new("reply_args_v2");

pub const SINGLE_STEP_REPLY_ARGS: Item<ReplyArgs> = Item::new("single_step_reply_args_v2");

pub const CONVERT_REPLY_ARGS: Item<ConvertReplyArgs> = Item::new("convert_reply_args_v2");

/// static info on one channel that doesn't change
pub const CHANNEL_INFO: Map<&str, ChannelInfo> = Map::new("channel_info");

// /// Forward channel state is used when LOCAL chain initiates ibc transfer to remote chain
// pub const CHANNEL_FORWARD_STATE: Map<(&str, &str), ChannelState> =
//     Map::new("channel_forward_state");

/// Reverse channel state is used when REMOTE chain initiates ibc transfer to local chain
pub const CHANNEL_REVERSE_STATE: Map<(&str, &str), ChannelState> =
    Map::new("channel_reverse_state");

/// Reverse channel state is used when LOCAL chain initiates ibc transfer to remote chain
// pub const CHANNEL_FORWARD_STATE: Map<(&str, &str), ChannelState> =
// Map::new("channel_forward_state");

/// Every cw20 contract we allow to be sent is stored here, possibly with a gas_limit
pub const ALLOW_LIST: Map<&Addr, AllowInfo> = Map::new("allow_list");

pub const TOKEN_FEE: Map<&str, Ratio> = Map::new("token_fee");

// relayer fee. This fee depends on the network type, not token type
// decimals of relayer fee should always be 10^6 because we use ORAI as relayer fee
pub const RELAYER_FEE: Map<&str, Uint128> = Map::new("relayer_fee");

// store refund info 
pub const REFUND_INFO_LIST: Item<Vec<RefundInfo>> = Item::new("refund_info_list");

// store temp refund info, will be remove when store to REFUND_INFO_LIST
pub const REFUND_INFO: Item<Option<RefundInfo>> = Item::new("refund_info");

// refund info store refund information when packet failed
#[cw_serde]
pub struct RefundInfo {
    pub receiver: String,
    pub amount: Amount
}

// // accumulated token fee
// pub const TOKEN_FEE_ACCUMULATOR: Map<&str, Uint128> = Map::new("token_fee_accumulator");

// // accumulated relayer fee
// pub const RELAYER_FEE_ACCUMULATOR: Map<&str, Uint128> = Map::new("relayer_fee_accumulator");

// MappingMetadataIndexex structs keeps a list of indexers
pub struct MappingMetadataIndexex<'a> {
    // token.identifier
    pub asset_info: MultiIndex<'a, String, MappingMetadata, String>,
}

// IndexList is just boilerplate code for fetching a struct's indexes
impl<'a> IndexList<MappingMetadata> for MappingMetadataIndexex<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<MappingMetadata>> + '_> {
        let v: Vec<&dyn Index<MappingMetadata>> = vec![&self.asset_info];
        Box::new(v.into_iter())
    }
}

///  used when chain A (no cosmwasm) sends native token to chain B (has cosmwasm). key - original denom of chain A,
/// in form of ibc no hash for destination port & channel - transfer/channel-0/uatom for example; value - mapping data
/// including asset info, can be either native or cw20
pub fn ics20_denoms<'a>() -> IndexedMap<'a, &'a str, MappingMetadata, MappingMetadataIndexex<'a>> {
    let indexes = MappingMetadataIndexex {
        asset_info: MultiIndex::new(
            |_k, d| d.asset_info.to_string(),
            "ics20_mapping_namespace",
            "asset__info",
        ),
    };
    IndexedMap::new("ics20_mapping_namespace", indexes)
}

#[cw_serde]
#[derive(Default)]
pub struct ChannelState {
    pub outstanding: Uint128,
    pub total_sent: Uint128,
}

#[cw_serde]
pub struct Config {
    pub default_timeout: u64,
    pub default_gas_limit: Option<u64>,
    pub fee_denom: String,
    pub swap_router_contract: RouterController,
    pub token_fee_receiver: Addr,
    pub relayer_fee_receiver: Addr,
    pub converter_contract: ConverterController,
    pub osor_entrypoint_contract: String,
    pub token_factory_addr: Addr,
}

pub fn increase_channel_balance(
    storage: &mut dyn Storage,
    channel: &str,
    denom: &str, // should be ibc denom
    amount: Uint128,
) -> Result<(), ContractError> {
    let store = CHANNEL_REVERSE_STATE.key((channel, denom));
    // whatever error or not found, return default
    let mut state = store.load(storage).unwrap_or_default();
    state.outstanding += amount;
    state.total_sent += amount;
    store.save(storage, &state).map_err(ContractError::Std)
}

pub fn reduce_channel_balance(
    storage: &mut dyn Storage,
    channel: &str,
    denom: &str, // should be ibc denom
    amount: Uint128,
) -> Result<(), ContractError> {
    let store = CHANNEL_REVERSE_STATE.key((channel, denom));
    let Ok(mut state) = store.load(storage) else {
        return Err(ContractError::NoSuchChannelState {
            id: channel.to_string(),
            denom: denom.to_string(),
        });
    };

    state.outstanding =
        state
            .outstanding
            .checked_sub(amount)
            .map_err(|_| ContractError::InsufficientFunds {
                id: channel.to_string(),
                denom: denom.to_string(),
            })?;

    store.save(storage, &state).map_err(ContractError::Std)
}

// only used for admin of the contract
pub fn override_channel_balance(
    storage: &mut dyn Storage,
    channel: &str,
    denom: &str, // should be ibc denom
    outstanding: Uint128,
    total_sent: Option<Uint128>,
) -> Result<(), ContractError> {
    CHANNEL_REVERSE_STATE.update(storage, (channel, denom), |orig| -> StdResult<_> {
        let mut state = orig.unwrap_or_default();
        state.outstanding = outstanding;
        if let Some(total_sent) = total_sent {
            state.total_sent = total_sent;
        }
        Ok(state)
    })?;
    Ok(())
}

// this is like increase, but it only "un-subtracts" (= adds) outstanding, not total_sent
// calling `reduce_channel_balance` and then `undo_reduce_channel_balance` should leave state unchanged.
pub fn undo_reduce_channel_balance(
    storage: &mut dyn Storage,
    channel: &str,
    denom: &str,
    amount: Uint128,
) -> Result<(), ContractError> {
    CHANNEL_REVERSE_STATE.update(storage, (channel, denom), |orig| -> StdResult<_> {
        let mut state = orig.unwrap_or_default();
        state.outstanding += amount;
        Ok(state)
    })?;
    Ok(())
}

pub fn get_key_ics20_ibc_denom(port_id: &str, channel_id: &str, denom: &str) -> String {
    format!("{}/{}/{}", port_id, channel_id, denom)
}
