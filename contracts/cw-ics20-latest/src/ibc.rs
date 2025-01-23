use std::ops::Mul;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    attr, entry_point, from_json, to_json_binary, wasm_execute, Api, Binary, Coin, CosmosMsg,
    Decimal, Deps, DepsMut, Env, Ibc3ChannelOpenResponse, IbcBasicResponse, IbcChannel,
    IbcChannelCloseMsg, IbcChannelConnectMsg, IbcChannelOpenMsg, IbcEndpoint, IbcMsg, IbcOrder,
    IbcPacket, IbcPacketAckMsg, IbcPacketReceiveMsg, IbcPacketTimeoutMsg, IbcReceiveResponse,
    IbcTimeout, Order, QuerierWrapper, Reply, Response, StdError, StdResult, Storage, SubMsg,
    SubMsgResult, Uint128,
};

use cw20_ics20_msg::helper::{
    denom_to_asset_info, get_full_denom, get_prefix_decode_bech32, parse_asset_info_denom,
};
use cw_storage_plus::Map;
use oraiswap::asset::AssetInfo;
use oraiswap::router::{RouterController, SwapOperation};
use skip::entry_point::ExecuteMsg as EntryPointExecuteMsg;
use token_bindings::Metadata;

use crate::contract::build_mint_mapping_msg;
use crate::error::{ContractError, Never};
use crate::msg::{ExecuteMsg, RegisterDenomMsg};
use crate::state::{
    get_key_ics20_ibc_denom, ics20_denoms, undo_reduce_channel_balance, RefundInfo, ALLOW_LIST, CHANNEL_INFO, CONFIG, REFUND_INFO_LIST, RELAYER_FEE, TEMP_REFUND_INFO, TOKEN_FEE
};
use cw20_ics20_msg::amount::{convert_remote_to_local, Amount};
use cw20_ics20_msg::msg::FeeData;
use cw20_ics20_msg::state::{ChannelInfo, MappingMetadata, Ratio};

pub const ICS20_VERSION: &str = "ics20-1";
pub const ICS20_ORDERING: IbcOrder = IbcOrder::Unordered;
pub const ORAIBRIDGE_PREFIX: &str = "oraib";

/// The format for sending an ics20 packet.
/// Proto defined here: https://github.com/cosmos/cosmos-sdk/blob/v0.42.0/proto/ibc/applications/transfer/v1/transfer.proto#L11-L20
/// This is compatible with the JSON serialization
#[cw_serde]
pub struct Ics20Packet {
    /// amount of tokens to transfer is encoded as a string
    pub amount: Uint128,
    /// the token denomination to be transferred
    pub denom: String,
    /// the recipient address on the destination chain
    pub receiver: String,
    /// the sender address
    pub sender: String,
    /// optional memo
    pub memo: Option<String>,
}

impl Ics20Packet {
    pub fn new<T: Into<String>>(
        amount: Uint128,
        denom: T,
        sender: &str,
        receiver: &str,
        memo: Option<String>,
    ) -> Self {
        Ics20Packet {
            denom: denom.into(),
            amount,
            sender: sender.to_string(),
            receiver: receiver.to_string(),
            memo,
        }
    }
}

/// This is a generic ICS acknowledgement format.
/// Proto defined here: https://github.com/cosmos/cosmos-sdk/blob/v0.42.0/proto/ibc/core/channel/v1/channel.proto#L141-L147
/// This is compatible with the JSON serialization
#[cw_serde]
pub enum Ics20Ack {
    Result(Binary),
    Error(String),
}

// create a serialized success message
fn ack_success() -> Binary {
    let res = Ics20Ack::Result(b"1".into());
    to_json_binary(&res).unwrap()
}

// create a serialized error message
pub fn ack_fail(err: String) -> Binary {
    let res = Ics20Ack::Error(err);
    to_json_binary(&res).unwrap()
}

pub const NATIVE_RECEIVE_ID: u64 = 1338;
pub const REFUND_FAILURE_ID: u64 = 1340;
pub const UNIVERSAL_SWAP_ERROR_ID: u64 = 1344;

#[entry_point]
pub fn reply(deps: DepsMut, _env: Env, reply: Reply) -> Result<Response, ContractError> {
    // if let SubMsgResult::Err(err) = reply.result {
    //     return match reply.id {
    //         // happens only when send cw20 amount to recipient failed. Wont refund because this case is unlikely to happen
    //         NATIVE_RECEIVE_ID => Ok(Response::new()
    //             .set_data(ack_success())
    //             .add_attribute("action", "native_receive_id")
    //             .add_attribute("error_transferring_ibc_tokens_to_cw20", err)),
    //         // fallback case when refund fails. Wont retry => will refund manually
    //         REFUND_FAILURE_ID => {
    //             // we all set ack success so that this token is stuck on Oraichain, not on OraiBridge because if ack fail => token refunded on OraiBridge yet still refund on Oraichain
    //             Ok(Response::new()
    //                 .set_data(ack_success())
    //                 .add_attribute("action", "refund_failure_id")
    //                 .add_attribute("error_trying_to_refund_single_step", err))
    //         }
    //         // fallback case when refund fails. Wont retry => will refund manually
    //         UNIVERSAL_SWAP_ERROR_ID => {
    //             // we all set ack success so that this token is stuck on Oraichain, not on OraiBridge because if ack fail => token refunded on OraiBridge yet still refund on Oraichain
    //             Ok(Response::new()
    //                 .set_data(ack_success())
    //                 .add_attribute("action", "universal_swap_error")
    //                 .add_attribute("error_trying_to_call_entrypoint_for_universal_swap", err))
    //         }
    //         _ => Err(ContractError::UnknownReplyId { id: reply.id }),
    //     };
    // }
    match reply.result {
        SubMsgResult::Err(err) => handle_reply_error(deps, err, reply.id),
        SubMsgResult::Ok(_) => handle_reply_success(deps, reply.id),
    }
}

fn handle_reply_error(deps: DepsMut, err: String, id: u64) -> Result<Response, ContractError> {
    match id {
        NATIVE_RECEIVE_ID => {
            // add packet to refund array
            if let Some(packet_sent) = TEMP_REFUND_INFO.load(deps.storage).unwrap() {
                // remove relay packet store
                TEMP_REFUND_INFO.save(deps.storage, &None)?;

                // store packet into refund list
                REFUND_INFO_LIST.update(deps.storage, |mut lists| -> StdResult<_> {
                    lists.push(packet_sent);
                    StdResult::Ok(lists)
                })?; 
            }
            
            Ok(Response::new()
                .set_data(ack_success())
                .add_attribute("action", "native_receive_id")
                .add_attribute("error_transferring_ibc_tokens_to_cw20", err))
        },

        REFUND_FAILURE_ID => {
            // add packet to refund array
            if let Some(packet_sent) = TEMP_REFUND_INFO.load(deps.storage).unwrap() {
                // remove relay packet store
                TEMP_REFUND_INFO.save(deps.storage, &None)?;

                // store packet into refund list
                REFUND_INFO_LIST.update(deps.storage, |mut lists| -> StdResult<_> {
                    lists.push(packet_sent);
                    StdResult::Ok(lists)
                })?; 
            }

            Ok(Response::new()
                .set_data(ack_success())
                .add_attribute("action", "refund_failure_id")
                .add_attribute("error_trying_to_refund_single_step", err))

        },

        UNIVERSAL_SWAP_ERROR_ID => {
            // we all set ack success so that this token is stuck on Oraichain, not on OraiBridge because if ack fail => token refunded on OraiBridge yet still refund on Oraichain
            Ok(Response::new()
                .set_data(ack_success())
                .add_attribute("action", "universal_swap_error")
                .add_attribute("error_trying_to_call_entrypoint_for_universal_swap", err))
        },
        
        _ => Err(ContractError::UnknownReplyId { id: id }),
    }
}

fn handle_reply_success(deps: DepsMut, id: u64) -> Result<Response, ContractError> {
    match id {
        NATIVE_RECEIVE_ID => {
            // in this case we simply remove temp refund info
            if let Some(_packet_sent) = TEMP_REFUND_INFO.load(deps.storage).unwrap() {
                // remove relay packet store
                TEMP_REFUND_INFO.save(deps.storage, &None)?;
            }
            
            Ok(Response::default())
        },

        REFUND_FAILURE_ID => {
            // in this case we simply remove temp refund info
            if let Some(_packet_sent) = TEMP_REFUND_INFO.load(deps.storage).unwrap() {
                // remove relay packet store
                TEMP_REFUND_INFO.save(deps.storage, &None)?;
            }

            Ok(Response::default())
        },

        _ => Err(ContractError::UnknownReplyId { id: id }),
    }
}

#[entry_point]
/// enforces ordering and versioning constraints
pub fn ibc_channel_open(
    _deps: DepsMut,
    _env: Env,
    msg: IbcChannelOpenMsg,
) -> Result<Option<Ibc3ChannelOpenResponse>, ContractError> {
    enforce_order_and_version(msg.channel(), msg.counterparty_version())?;
    Ok(None)
}

#[entry_point]
/// record the channel in CHANNEL_INFO
pub fn ibc_channel_connect(
    deps: DepsMut,
    _env: Env,
    msg: IbcChannelConnectMsg,
) -> Result<IbcBasicResponse, ContractError> {
    // we need to check the counter party version in try and ack (sometimes here)
    enforce_order_and_version(msg.channel(), msg.counterparty_version())?;

    let channel: IbcChannel = msg.into();
    let info = ChannelInfo {
        id: channel.endpoint.channel_id,
        counterparty_endpoint: channel.counterparty_endpoint,
        connection_id: channel.connection_id,
    };
    CHANNEL_INFO.save(deps.storage, &info.id, &info)?;

    Ok(IbcBasicResponse::default())
}

fn enforce_order_and_version(
    channel: &IbcChannel,
    counterparty_version: Option<&str>,
) -> Result<(), ContractError> {
    if channel.version != ICS20_VERSION {
        return Err(ContractError::InvalidIbcVersion {
            version: channel.version.clone(),
        });
    }
    if let Some(version) = counterparty_version {
        if version != ICS20_VERSION {
            return Err(ContractError::InvalidIbcVersion {
                version: version.to_string(),
            });
        }
    }
    if channel.order != ICS20_ORDERING {
        return Err(ContractError::OnlyUnorderedChannel {});
    }
    Ok(())
}

#[entry_point]
pub fn ibc_channel_close(
    _deps: DepsMut,
    _env: Env,
    _channel: IbcChannelCloseMsg,
) -> Result<IbcBasicResponse, ContractError> {
    // don't allow close channel
    Err(ContractError::CannotClose {})
}

#[entry_point]
/// Check to see if we have any balance here
/// We should not return an error if possible, but rather an acknowledgement of failure
pub fn ibc_packet_receive(
    deps: DepsMut,
    env: Env,
    msg: IbcPacketReceiveMsg,
) -> Result<IbcReceiveResponse, Never> {
    let packet = msg.packet;

    do_ibc_packet_receive(
        deps.storage,
        deps.api,
        &deps.querier,
        env,
        &packet,
        &msg.relayer.into_string(),
    )
    .or_else(|err| {
        Ok(IbcReceiveResponse::new()
            // trade-off between reentrancy & refunding. If error, then it should be a serious error => refund to oraibridge
            // that's better than trying to update balance & let it stay in this contract and expose to reentrancy
            .set_ack(ack_fail(err.to_string()))
            .add_attributes(vec![
                attr("action", "receive"),
                attr("success", "false"),
                attr("error", err.to_string()),
                attr("src_channel_id", packet.src.channel_id),
                attr("dst_channel_id", packet.dest.channel_id),
                attr("packet_data", packet.data.to_base64()),
            ]))
    })
}

// Returns local denom if the denom is an encoded voucher from the expected endpoint
// Otherwise, error
pub fn parse_voucher_denom<'a>(
    voucher_denom: &'a str,
    remote_endpoint: &IbcEndpoint,
) -> Result<(&'a str, bool), ContractError> {
    let split_denom: Vec<&str> = voucher_denom.splitn(3, '/').collect();

    // if it is a packet_receive of native token from chain A or IBC token that was sent from chain B.
    if split_denom.len() == 1 {
        return Ok((voucher_denom, true));
    }
    if split_denom.len() != 3 {
        return Err(ContractError::NoForeignTokens {});
    }
    // a few more sanity checks
    if split_denom[0] != remote_endpoint.port_id {
        return Err(ContractError::FromOtherPort {
            port: split_denom[0].into(),
        });
    }
    if split_denom[1] != remote_endpoint.channel_id {
        return Err(ContractError::FromOtherChannel {
            channel: split_denom[1].into(),
        });
    }

    Ok((split_denom[2], false))
}

// Returns local denom if the denom is an encoded voucher from the expected endpoint
// Otherwise, error
pub fn parse_ibc_denom_without_sanity_checks(ibc_denom: &str) -> StdResult<&str> {
    let split_denom: Vec<&str> = ibc_denom.splitn(3, '/').collect();

    if split_denom.len() != 3 {
        return Err(StdError::generic_err(
            ContractError::NoForeignTokens {}.to_string(),
        ));
    }
    Ok(split_denom[2])
}

// Returns
// Otherwise, error
pub fn parse_ibc_channel_without_sanity_checks(ibc_denom: &str) -> StdResult<&str> {
    let split_denom: Vec<&str> = ibc_denom.splitn(3, '/').collect();

    if split_denom.len() != 3 {
        return Err(StdError::generic_err(
            ContractError::NoForeignTokens {}.to_string(),
        ));
    }
    Ok(split_denom[1])
}

pub fn parse_ibc_info_without_sanity_checks(ibc_denom: &str) -> StdResult<(&str, &str, &str)> {
    let split_denom: Vec<&str> = ibc_denom.splitn(3, '/').collect();

    if split_denom.len() != 3 {
        return Err(StdError::generic_err(
            ContractError::NoForeignTokens {}.to_string(),
        ));
    }
    Ok((split_denom[0], split_denom[1], split_denom[2]))
}

// this does the work of ibc_packet_receive, we wrap it to turn errors into acknowledgements
fn do_ibc_packet_receive(
    storage: &mut dyn Storage,
    api: &dyn Api,
    querier: &QuerierWrapper,
    env: Env,
    packet: &IbcPacket,
    relayer: &str,
) -> Result<IbcReceiveResponse, ContractError> {
    let msg: Ics20Packet = from_json(&packet.data)?;

    // If the token originated on the remote chain, it looks like "ucosm".
    // If it originated on our chain, it looks like "port/channel/ucosm".
    let denom = parse_voucher_denom(&msg.denom, &packet.src)?;

    // if denom is native, we handle it the native way
    if denom.1 {
        return handle_ibc_packet_receive_native_remote_chain(
            storage, api, querier, env, denom.0, packet, &msg, relayer,
        );
    }

    Err(ContractError::Std(StdError::generic_err("Not supported")))
}

#[allow(clippy::too_many_arguments)]
fn handle_ibc_packet_receive_native_remote_chain(
    storage: &mut dyn Storage,
    api: &dyn Api,
    querier: &QuerierWrapper,
    env: Env,
    denom: &str,
    packet: &IbcPacket,
    msg: &Ics20Packet,
    relayer: &str,
) -> Result<IbcReceiveResponse, ContractError> {
    let config = CONFIG.load(storage)?;
    let mut cosmos_msgs: Vec<CosmosMsg> = vec![];
    let ibc_packet_amount = msg.amount.to_string();
    let attributes: Vec<(&str, &str)> = vec![
        ("action", "receive_native"),
        ("sender", &msg.sender),
        ("receiver", &msg.receiver),
        ("denom", denom),
        ("amount", &ibc_packet_amount),
        ("success", "true"),
        ("relayer", relayer),
    ];

    // key in form transfer/channel-0/foo
    let ibc_denom = get_key_ics20_ibc_denom(&packet.dest.port_id, &packet.dest.channel_id, denom);
    let pair_mapping = match ics20_denoms().load(storage, &ibc_denom) {
        Ok(pair_mapping) => pair_mapping,
        Err(_) => {
            let (prefix, denom) =
                denom
                    .split_once("0x")
                    .ok_or(ContractError::Std(StdError::GenericErr {
                        msg: String::from("Cannot parse denom"),
                    }))?;

            let bytes_address = hex::decode(denom).map_err(|_| {
                ContractError::Std(StdError::GenericErr {
                    msg: String::from("Invalid hex address"),
                })
            })?;
            let base58_address = bs58::encode(bytes_address).into_string();
            let base58_denom = format!("{}0x{}", prefix, base58_address);
            // push a register denom msg to the contract
            cosmos_msgs.push(
                wasm_execute(
                    env.contract.address.to_string(),
                    &ExecuteMsg::RegisterDenom(RegisterDenomMsg {
                        subdenom: base58_denom.clone(),
                        metadata: None,
                    }),
                    vec![Coin::new(1, "orai")],
                )?
                .into(),
            );
            let new_metadata = MappingMetadata {
                asset_info: AssetInfo::NativeToken {
                    denom: get_full_denom(config.token_factory_addr.to_string(), base58_denom),
                },
                remote_decimals: 1, // Since we don't know metadata of remote_chain token, we set it to 1 in both decimals
                asset_info_decimals: 1,
                is_mint_burn: true, // Always mint burn if we don't know the metadata
            };
            ics20_denoms().save(storage, &ibc_denom, &new_metadata)?;
            new_metadata
        }
    };
    let initial_receive_asset_info = pair_mapping.asset_info;
    let to_send = Amount::from_parts(
        parse_asset_info_denom(&initial_receive_asset_info),
        convert_remote_to_local(
            msg.amount,
            pair_mapping.remote_decimals,
            pair_mapping.asset_info_decimals,
        )?,
    );

    // increase channel balance submsg. We increase it first before doing other tasks
    cosmos_msgs.push(
        wasm_execute(
            env.contract.address.to_string(),
            &ExecuteMsg::IncreaseChannelBalanceIbcReceive {
                dest_channel_id: packet.dest.channel_id.clone(),
                ibc_denom: ibc_denom.clone(),
                amount: msg.amount,
                local_receiver: msg.receiver.clone(),
            },
            vec![],
        )?
        .into(),
    );

    let fee_data = process_deduct_fee(
        storage,
        querier,
        api,
        &msg.sender,
        &msg.denom,
        to_send.clone(),
        &config.swap_router_contract,
    )?;

    // if the fees have consumed all user funds, we send all the fees to our token fee receiver
    if fee_data.deducted_amount.is_zero() {
        return Ok(IbcReceiveResponse::new()
            .set_ack(ack_success())
            .add_messages(cosmos_msgs)
            .add_message(to_send.send_amount(config.token_fee_receiver.into_string(), None))
            .add_attributes(attributes)
            .add_attributes(vec![
                ("token_fee", &fee_data.token_fee.amount().to_string()),
                ("relayer_fee", &fee_data.relayer_fee.amount().to_string()),
            ]));
    }
    if !fee_data.token_fee.is_empty() {
        cosmos_msgs.push(
            fee_data
                .token_fee
                .send_amount(config.token_fee_receiver.into_string(), None),
        )
    }
    if !fee_data.relayer_fee.is_empty() {
        cosmos_msgs.push(fee_data.relayer_fee.send_amount(relayer.to_string(), None))
    }
    let new_deducted_to_send = Amount::from_parts(to_send.denom(), fee_data.deducted_amount);
    let sub_msgs = get_follow_up_msgs(
        storage,
        api,
        msg.receiver.clone(),
        new_deducted_to_send,
        msg.memo.clone(),
    )?;

    let res = IbcReceiveResponse::new()
        .set_ack(ack_success())
        .add_messages(cosmos_msgs)
        .add_submessages(sub_msgs)
        .add_attributes(attributes)
        .add_attributes(vec![
            ("token_fee", &fee_data.token_fee.amount().to_string()),
            ("relayer_fee", &fee_data.relayer_fee.amount().to_string()),
        ]);

    Ok(res)
}

pub fn get_follow_up_msgs(
    storage: &mut dyn Storage,
    api: &dyn Api,
    orai_receiver: String,
    to_send: Amount,
    memo: Option<String>,
) -> Result<Vec<SubMsg>, ContractError> {
    let config = CONFIG.load(storage)?;
    let mut sub_msgs: Vec<SubMsg> = vec![];
    let send_only_sub_msg =
        SubMsg::reply_always(to_send.send_amount(orai_receiver.clone(), None), NATIVE_RECEIVE_ID);
    if let Some(memo) = memo {
        // Do not call universal swap if the memo is empty or is an address.
        if memo.is_empty() || api.addr_validate(&memo).is_ok() {
            sub_msgs.push(send_only_sub_msg);

            // add this submsg to store
            let refund_info = RefundInfo{
                receiver: orai_receiver,
                amount: to_send,
            };
            TEMP_REFUND_INFO.save(storage, &Some(refund_info))?;
        } else {
            let swap_then_post_action_msg = to_send.send_amount(
                config.osor_entrypoint_contract,
                Some(to_json_binary(&EntryPointExecuteMsg::UniversalSwap {
                    memo,
                })?),
            );
            // currently, we not auto refund with universal swap
            let sub_msg =
                SubMsg::reply_on_error(swap_then_post_action_msg, UNIVERSAL_SWAP_ERROR_ID);
            sub_msgs.push(sub_msg);
        }
    } else {
        sub_msgs.push(send_only_sub_msg);
        // add this submsg to store
        let refund_info = RefundInfo{
            receiver: orai_receiver,
            amount: to_send,
        };
        TEMP_REFUND_INFO.save(storage, &Some(refund_info))?;
    }
    Ok(sub_msgs)
}

pub fn check_gas_limit(deps: Deps, amount: &Amount) -> Result<Option<u64>, ContractError> {
    match amount {
        Amount::Cw20(coin) => {
            // if cw20 token, use the registered gas limit, or error if not whitelisted
            let addr = deps.api.addr_validate(coin.address.as_str())?;
            let allowed = ALLOW_LIST.may_load(deps.storage, &addr)?;
            match allowed {
                Some(allow) => Ok(allow.gas_limit),
                None => match CONFIG.load(deps.storage)?.default_gas_limit {
                    Some(base) => Ok(Some(base)),
                    None => Err(ContractError::NotOnAllowList),
                },
            }
        }
        _ => Ok(None),
    }
}

pub fn process_deduct_fee(
    storage: &mut dyn Storage,
    querier: &QuerierWrapper,
    api: &dyn Api,
    remote_sender: &str,
    remote_token_denom: &str,
    local_amount: Amount, // local amount
    swap_router_contract: &RouterController,
) -> StdResult<FeeData> {
    let local_denom = local_amount.denom();
    let (deducted_amount, token_fee) =
        deduct_token_fee(storage, remote_token_denom, local_amount.amount())?;

    let mut fee_data = FeeData {
        deducted_amount,
        token_fee: Amount::from_parts(local_denom.clone(), token_fee),
        relayer_fee: Amount::from_parts(local_denom.clone(), Uint128::zero()),
    };
    // if after token fee, the deducted amount is 0 then we deduct all to token fee
    if deducted_amount.is_zero() {
        fee_data.token_fee = local_amount;
        return Ok(fee_data);
    }

    // simulate for relayer fee
    let ask_asset_info = denom_to_asset_info(api, &local_amount.raw_denom());

    let relayer_fee = deduct_relayer_fee(
        storage,
        api,
        querier,
        remote_sender,
        remote_token_denom,
        ask_asset_info,
        swap_router_contract,
    )?;

    fee_data.deducted_amount = deducted_amount.checked_sub(relayer_fee).unwrap_or_default();
    fee_data.relayer_fee = Amount::from_parts(local_denom.clone(), relayer_fee);
    // if the relayer fee makes the final amount 0, then we charge the remaining deducted amount as relayer fee
    if fee_data.deducted_amount.is_zero() {
        fee_data.relayer_fee = Amount::from_parts(local_denom.clone(), deducted_amount);
    }
    Ok(fee_data)
}

pub fn deduct_token_fee(
    storage: &mut dyn Storage,
    remote_token_denom: &str,
    amount: Uint128,
) -> StdResult<(Uint128, Uint128)> {
    let token_fee = TOKEN_FEE.may_load(storage, remote_token_denom)?;
    if let Some(token_fee) = token_fee {
        let fee = deduct_fee(token_fee, amount);
        let new_deducted_amount = amount.checked_sub(fee)?;
        return Ok((new_deducted_amount, fee));
    }
    Ok((amount, Uint128::zero()))
}

pub fn deduct_relayer_fee(
    storage: &mut dyn Storage,
    _api: &dyn Api,
    querier: &QuerierWrapper,
    remote_address: &str,
    remote_token_denom: &str,
    ask_asset_info: AssetInfo,
    swap_router_contract: &RouterController,
) -> StdResult<Uint128> {
    // this is bech32 prefix of sender from other chains. Should not error because we are in the cosmos ecosystem. Every address should have prefix
    // evm case, need to filter remote token denom since prefix is always oraib
    let prefix_result = get_prefix_decode_bech32(remote_address);

    let prefix: String = match prefix_result {
        Err(_) => convert_remote_denom_to_evm_prefix(remote_token_denom),
        Ok(prefix) => {
            if prefix.eq(ORAIBRIDGE_PREFIX) {
                convert_remote_denom_to_evm_prefix(remote_token_denom)
            } else {
                prefix
            }
        }
    };
    let relayer_fee = RELAYER_FEE.may_load(storage, &prefix)?;
    // no need to deduct fee if no fee is found in the mapping
    Ok(relayer_fee
        .map(|offer_amount| {
            get_swap_token_amount_out_from_orai(
                querier,
                offer_amount,
                swap_router_contract,
                ask_asset_info,
            )
        })
        .unwrap_or_default())
}

pub fn deduct_fee(token_fee: Ratio, amount: Uint128) -> Uint128 {
    // ignore case where denominator is zero since we cannot divide with 0
    if token_fee.denominator == 0 {
        return Uint128::zero();
    }
    amount.mul(Decimal::from_ratio(
        token_fee.nominator,
        token_fee.denominator,
    ))
}

pub fn get_swap_token_amount_out_from_orai(
    querier: &QuerierWrapper,
    offer_amount: Uint128,
    swap_router_contract: &RouterController,
    ask_asset_info: AssetInfo,
) -> Uint128 {
    let orai_asset_info = AssetInfo::NativeToken {
        denom: "orai".to_string(),
    };
    if ask_asset_info.eq(&orai_asset_info) {
        return offer_amount;
    }
    // TODO: switch this to mixed router?
    swap_router_contract
        .simulate_swap(
            querier,
            offer_amount,
            vec![SwapOperation::OraiSwap {
                offer_asset_info: orai_asset_info,
                // always swap with orai. If it does not share a pool with ORAI => ignore, no fee
                ask_asset_info,
            }],
        )
        .map(|data| data.amount)
        .unwrap_or_default()
}

pub fn convert_remote_denom_to_evm_prefix(remote_denom: &str) -> String {
    match remote_denom.split_once("0x") {
        Some((evm_prefix, _)) => evm_prefix.to_string(),
        None => "".to_string(),
    }
}

pub fn collect_fee_msgs(
    storage: &mut dyn Storage,
    receiver: String,
    fee_accumulator: Map<&str, Uint128>,
) -> StdResult<Vec<CosmosMsg>> {
    let cosmos_msgs = fee_accumulator
        .range(storage, None, None, Order::Ascending)
        .filter_map(|data| {
            data.map(|fee_info| {
                if fee_info.1.is_zero() {
                    return None;
                }
                Some(Amount::from_parts(fee_info.0, fee_info.1).send_amount(receiver.clone(), None))
            })
            .ok()
        })
        .flatten()
        .collect::<Vec<_>>();
    // we reset all the accumulator keys to zero so that it wont accumulate more in the next txs. This action will be reverted if the fee payment txs fail.
    fee_accumulator.clear(storage);
    Ok(cosmos_msgs)
}

#[entry_point]
/// check if success or failure and update balance, or return funds
/// This entrypoint is called when we receive an acknowledgement packet from a remote chain
pub fn ibc_packet_ack(
    deps: DepsMut,
    _env: Env,
    msg: IbcPacketAckMsg,
) -> Result<IbcBasicResponse, ContractError> {
    // Design decision: should we trap error like in receive?
    // retried again and again. is that good?
    let ics20msg: Ics20Ack = from_json(&msg.acknowledgement.data)?;
    match ics20msg {
        Ics20Ack::Result(_) => on_packet_success(deps, msg.original_packet),
        Ics20Ack::Error(err) => on_packet_failure(deps, msg.original_packet, err),
    }
}

#[entry_point]
/// return fund to original sender (same as failure in ibc_packet_ack)
pub fn ibc_packet_timeout(
    deps: DepsMut,
    _env: Env,
    msg: IbcPacketTimeoutMsg,
) -> Result<IbcBasicResponse, ContractError> {
    let packet = msg.packet;
    on_packet_failure(deps, packet, "timeout".to_string())
}

// update the balance stored on this (channel, denom) index
fn on_packet_success(_deps: DepsMut, packet: IbcPacket) -> Result<IbcBasicResponse, ContractError> {
    let msg: Ics20Packet = from_json(packet.data)?;

    // similar event messages like ibctransfer module
    let attributes = vec![
        attr("action", "acknowledge"),
        attr("sender", &msg.sender),
        attr("receiver", &msg.receiver),
        attr("denom", &msg.denom),
        attr("amount", msg.amount),
        attr("success", "true"),
    ];

    // if let Some(memo) = msg.memo {
    //     attributes.push(attr("memo", memo));
    // }

    Ok(IbcBasicResponse::new().add_attributes(attributes))
}

// return the tokens to sender
// only gets called when we receive an acknowledgement packet from the remote chain
// it means that the ibc packet we sent must be successful, but there's something wrong with the remote chain that they cannot receive a successful acknowledgement
// will refund because this case is different from the FOLLOW_UP_IBC_SEND_FAILURE_ID
// FOLLOW_UP_IBC_SEND_FAILURE_ID failed to send ibc packet. This one has successfully sent
fn on_packet_failure(
    deps: DepsMut,
    packet: IbcPacket,
    err: String,
) -> Result<IbcBasicResponse, ContractError> {
    let msg: Ics20Packet = from_json(&packet.data)?;

    // in case that the denom is not in the mapping list, meaning that it is not transferred back, but transfer originally from this local chain
    if ics20_denoms().may_load(deps.storage, &msg.denom)?.is_none() {
        return Ok(IbcBasicResponse::new());
    }

    let sub_msg = handle_packet_refund(deps.storage, &msg.sender, &msg.denom, msg.amount, true)?;
    // since we reduce the channel's balance optimistically when transferring back, we undo reduce it again when receiving failed ack
    undo_reduce_channel_balance(deps.storage, &packet.src.channel_id, &msg.denom, msg.amount)?;

    let res = IbcBasicResponse::new()
        .add_submessage(sub_msg)
        .add_attribute("action", "acknowledge")
        .add_attribute("sender", msg.sender)
        .add_attribute("receiver", msg.receiver)
        .add_attribute("denom", msg.denom)
        .add_attribute("amount", msg.amount.to_string())
        .add_attribute("success", "false")
        .add_attribute("error", err);

    Ok(res)

    // send ack fail to custom contract for refund
}

pub fn handle_packet_refund(
    storage: &mut dyn Storage,
    packet_sender: &str,
    packet_denom: &str,
    packet_amount: Uint128,
    with_mint_burn: bool,
) -> Result<SubMsg, ContractError> {
    // get ibc denom mapping to get cw20 denom & from decimals in case of packet failure, we can refund the corresponding user & amount
    let pair_mapping = ics20_denoms().load(storage, packet_denom)?;
    let config = CONFIG.load(storage)?;

    let local_amount = convert_remote_to_local(
        packet_amount,
        pair_mapping.remote_decimals,
        pair_mapping.asset_info_decimals,
    )?;

    // check if mint_burn mechanism, then mint token for packet sender, if not, send from contract
    let denom = parse_asset_info_denom(&pair_mapping.asset_info);
    let send_amount_msg = Amount::from_parts(
        denom.clone(),
        local_amount,
    )
    .send_amount(packet_sender.to_string(), None);
    let cosmos_msg = match build_mint_mapping_msg(
        config.token_factory_addr.to_string(),
        pair_mapping.is_mint_burn,
        pair_mapping.asset_info,
        local_amount,
        packet_sender.to_string(),
    )? {
        Some(cosmos_msg) => {
            if with_mint_burn {
                cosmos_msg
            } else {
                send_amount_msg
            }
        }
        None => send_amount_msg,
    };

    // save temp refund info, we use this info in handle reply enpoint
    let temp_refund_info = RefundInfo{
        amount:  Amount::from_parts(
            denom,
            local_amount,
        ),
        receiver: packet_sender.to_string()
    };
    TEMP_REFUND_INFO.save(storage, &Some(temp_refund_info))?;

    // used submsg here & reply on error. This means that if the refund process fails => tokens will be locked in this IBC Wasm contract. We will manually handle that case. No retry
    // similar event messages like ibctransfer module
    Ok(SubMsg::reply_always(cosmos_msg, REFUND_FAILURE_ID))
}

pub fn build_ibc_send_packet(
    amount: Uint128,
    denom: &str,
    sender: &str,
    receiver: &str,
    memo: Option<String>,
    src_channel: &str,
    timeout: IbcTimeout,
) -> StdResult<IbcMsg> {
    // build ics20 packet
    let packet = Ics20Packet::new(
        amount,
        denom, // we use ibc denom in form <transfer>/<channel>/<denom> so that when it is sent back to remote chain, it gets parsed correctly and burned
        sender, receiver, memo,
    );

    // prepare ibc message
    Ok(IbcMsg::SendPacket {
        channel_id: src_channel.to_string(),
        data: to_json_binary(&packet)?,
        timeout,
    })
}
