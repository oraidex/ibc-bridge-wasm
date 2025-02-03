use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Uint128};
use cw20::Cw20ReceiveMsg;
use cw20_ics20_msg::msg::{AllowedInfo, DeletePairMsg, TransferBackMsg, UpdatePairMsg};
use oraiswap::asset::AssetInfo;

use cw20_ics20_msg::state::{ChannelInfo, MappingMetadata, Ratio, RelayerFee, TokenFee};
use cw20_ics20_msg::{amount::Amount, ibc_hooks::HookMethods};
use token_bindings::Metadata;
use crate::state::{RefundInfo};

#[cw_serde]
pub struct InitMsg {
    /// Default timeout for ics20 packets, specified in seconds
    pub default_timeout: u64,
    /// who can allow more contracts
    pub gov_contract: String,
    /// initial allowlist - all cw20 tokens we will send must be previously allowed by governance
    pub allowlist: Vec<AllowMsg>,
    /// If set, contracts off the allowlist will run with this gas limit.
    /// If unset, will refuse to accept any contract off the allow list.
    pub default_gas_limit: Option<u64>,
    /// router contract for fee swap
    pub swap_router_contract: String,
    /// converter contract for convert token
    pub converter_contract: String,
    // entrypoint for handling swap and post actions like IBC transfer to remote
    pub osor_entrypoint_contract: String,
    // token factory proxy address
    pub token_factory_addr: String,
}

#[cw_serde]
pub struct AllowMsg {
    pub contract: String,
    pub gas_limit: Option<u64>,
}

#[cw_serde]
pub struct MigrateMsg {}

#[cw_serde]
pub enum ExecuteMsg {
    /// This accepts a properly-encoded ReceiveMsg from a cw20 contract
    Receive(Cw20ReceiveMsg),
    /// This allows us to transfer *exactly one* native token
    // Transfer(TransferMsg),
    TransferToRemote(TransferBackMsg),
    UpdateMappingPair(UpdatePairMsg),
    DeleteMappingPair(DeletePairMsg),
    /// This must be called by gov_contract, will allow a new cw20 token to be sent
    // Allow(AllowMsg),
    /// Change the admin (must be called by current admin)
    UpdateConfig {
        admin: Option<String>,
        default_timeout: Option<u64>,
        default_gas_limit: Option<u64>,
        swap_router_contract: Option<String>,
        token_fee: Option<Vec<TokenFee>>,
        relayer_fee: Option<Vec<RelayerFee>>,
        fee_receiver: Option<String>,
        relayer_fee_receiver: Option<String>,
        converter_contract: Option<String>,
        osor_entrypoint_contract: Option<String>,
        token_factory_addr: Option<String>,
    },
    // self-call msgs to deal with on_ibc_receive reentrancy error
    IncreaseChannelBalanceIbcReceive {
        dest_channel_id: String,
        ibc_denom: String,
        amount: Uint128,
        local_receiver: String,
    },
    ReduceChannelBalanceIbcReceive {
        src_channel_id: String,
        ibc_denom: String,
        amount: Uint128,
        local_receiver: String,
    },
    OverrideChannelBalance {
        channel_id: String,
        ibc_denom: String,
        outstanding: Uint128,
        total_sent: Option<Uint128>,
    },
    IbcHooksReceive {
        func: HookMethods,
        orai_receiver: String,
        args: Binary,
    },
    RegisterDenom(RegisterDenomMsg),
    WithdrawAsset {
        coin: Amount,
        receiver: Option<Addr>,
    },
}

#[cw_serde]
pub struct RegisterDenomMsg {
    pub subdenom: String,
    pub metadata: Option<Metadata>,
}

/// This is the message we accept via Receive
// #[cw_serde]
// pub struct TransferMsg {
//     /// The local channel to send the packets on
//     pub channel: String,
//     /// The remote address to send to.
//     /// Don't use HumanAddress as this will likely have a different Bech32 prefix than we use
//     /// and cannot be validated locally
//     pub remote_address: String,
//     /// How long the packet lives in seconds. If not specified, use default_timeout
//     pub timeout: Option<u64>,
//     /// metadata of the transfer to suit the new fungible token transfer
//     pub memo: Option<String>,
// }

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Return the port ID bound by this contract.
    #[returns(PortResponse)]
    Port {},
    /// Show all channels we have connected to.
    #[returns(ListChannelsResponse)]
    ListChannels {},
    /// Returns the details of the name channel, error if not created.
    #[returns(ChannelResponse)]
    Channel { id: String },
    /// Returns the details of the name channel, error if not created.
    #[returns(ChannelWithKeyResponse)]
    ChannelWithKey { channel_id: String, denom: String },
    /// Show the Config.
    #[returns(ConfigResponse)]
    Config {},
    #[returns(cw_controllers::AdminResponse)]
    Admin {},
    /// Query if a given cw20 contract is allowed.
    #[returns(AllowedResponse)]
    Allowed { contract: String },
    /// List all allowed cw20 contracts.
    #[returns(ListAllowedResponse)]
    ListAllowed {
        start_after: Option<String>,
        limit: Option<u32>,
        order: Option<u8>,
    },
    #[returns(cosmwasm_std::Addr)]
    #[returns(ListMappingResponse)]
    PairMappings {
        start_after: Option<String>,
        limit: Option<u32>,
        order: Option<u8>,
    },
    #[returns(PairQuery)]
    PairMapping { key: String },
    #[returns(Vec<PairQuery>)]
    PairMappingsFromAssetInfo { asset_info: AssetInfo },
    #[returns(Ratio)]
    GetTransferTokenFee { remote_token_denom: String },
    #[returns(Vec<RefundInfo>)]
    RefundInfoList {},
}

#[cw_serde]
pub struct SudoMsg{}

#[cw_serde]
pub struct ListChannelsResponse {
    pub channels: Vec<ChannelInfo>,
}

#[cw_serde]
pub struct ChannelResponse {
    /// Information on the channel's connection
    pub info: ChannelInfo,
    /// How many tokens we currently have pending over this channel
    pub balances: Vec<Amount>,
    /// The total number of tokens that have been sent over this channel
    /// (even if many have been returned, so balance is low)
    pub total_sent: Vec<Amount>,
}

#[cw_serde]
pub struct ChannelWithKeyResponse {
    /// Information on the channel's connection
    pub info: ChannelInfo,
    /// How many tokens we currently have pending over this channel
    pub balance: Amount,
    /// The total number of tokens that have been sent over this channel
    /// (even if many have been returned, so balance is low)
    pub total_sent: Amount,
}

#[cw_serde]
pub struct PortResponse {
    pub port_id: String,
}

#[cw_serde]
pub struct ConfigResponse {
    pub default_timeout: u64,
    pub default_gas_limit: Option<u64>,
    pub fee_denom: String,
    pub swap_router_contract: String,
    pub gov_contract: String,
    pub token_fee_receiver: Addr,
    pub relayer_fee_receiver: Addr,
    pub token_fees: Vec<TokenFee>,
    pub relayer_fees: Vec<RelayerFeeResponse>,
    pub converter_contract: String,
    pub osor_entrypoint_contract: String,
}

#[cw_serde]
pub struct RelayerFeeResponse {
    pub prefix: String,
    pub amount: Uint128,
}

#[cw_serde]
pub struct AllowedResponse {
    pub is_allowed: bool,
    pub gas_limit: Option<u64>,
}

#[cw_serde]
pub struct ListAllowedResponse {
    pub allow: Vec<AllowedInfo>,
}

#[cw_serde]
pub struct ListMappingResponse {
    pub pairs: Vec<PairQuery>,
}

#[cw_serde]
pub struct PairQuery {
    pub key: String,
    pub pair_mapping: MappingMetadata,
}
