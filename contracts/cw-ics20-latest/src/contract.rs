#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_json, to_json_binary, wasm_execute, Addr, Binary, CosmosMsg, Deps, DepsMut, Empty, Env,
    IbcEndpoint, IbcQuery, MessageInfo, Order, PortIdResponse, Response, StdError, StdResult,
    Storage, Timestamp, Uint128,
};
use cw2::set_contract_version;
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};
use cw20_ics20_msg::converter::ConverterController;
use cw20_ics20_msg::helper::parse_ibc_wasm_port_id;
use cw_controllers::AdminError;
use cw_storage_plus::Bound;
use oraiswap::asset::AssetInfo;
use oraiswap::router::RouterController;

use crate::error::ContractError;
use crate::ibc::{build_ibc_send_packet, parse_voucher_denom, process_deduct_fee};
use crate::ibc_hooks::ibc_hooks_receive;
use crate::msg::{
    AllowedResponse, ChannelResponse, ChannelWithKeyResponse, ConfigResponse, ExecuteMsg, InitMsg,
    ListAllowedResponse, ListChannelsResponse, ListMappingResponse, MigrateMsg, PairQuery,
    PortResponse, QueryMsg, RegisterDenomMsg, RelayerFeeResponse, SudoMsg,
};
use crate::query_helper::get_mappings_from_asset_info;
use crate::state::{
    get_key_ics20_ibc_denom, ics20_denoms, increase_channel_balance, override_channel_balance,
    reduce_channel_balance, Config, ADMIN, ALLOW_LIST, CHANNEL_INFO,
    CHANNEL_REVERSE_STATE, CONFIG, REFUND_INFO_LIST, RELAYER_FEE, REPLY_ARGS,
    SINGLE_STEP_REPLY_ARGS, TOKEN_FEE,
};
use cw20_ics20_msg::amount::{convert_local_to_remote, convert_remote_to_local, Amount};
use cw20_ics20_msg::msg::{AllowedInfo, DeletePairMsg, TransferBackMsg, UpdatePairMsg};
use cw20_ics20_msg::state::{AllowInfo, MappingMetadata, RelayerFee, ReplyArgs, TokenFee};
use cw_utils::{maybe_addr, nonpayable, one_coin};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:cw20-ics20";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[entry_point]
pub fn instantiate(
    mut deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InitMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    let admin = deps.api.addr_validate(&msg.gov_contract)?;
    ADMIN.set(deps.branch(), Some(admin.clone()))?;
    let cfg = Config {
        default_timeout: msg.default_timeout,
        default_gas_limit: msg.default_gas_limit,
        fee_denom: "orai".to_string(),
        swap_router_contract: RouterController(msg.swap_router_contract),
        token_fee_receiver: admin.clone(),
        relayer_fee_receiver: admin,
        converter_contract: ConverterController(msg.converter_contract),
        osor_entrypoint_contract: msg.osor_entrypoint_contract,
        token_factory_addr: deps.api.addr_validate(&msg.token_factory_addr)?,
    };
    CONFIG.save(deps.storage, &cfg)?;

    let refund_list = vec![];
    REFUND_INFO_LIST.save(deps.storage, &refund_list)?;

    // add all allows
    for allowed in msg.allowlist {
        let contract = deps.api.addr_validate(&allowed.contract)?;
        let info = AllowInfo {
            gas_limit: allowed.gas_limit,
        };
        ALLOW_LIST.save(deps.storage, &contract, &info)?;
    }
    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Receive(msg) => execute_receive(deps, env, info, msg),
        // ExecuteMsg::Transfer(msg) => {
        //     let coin = one_coin(&info)?;
        //     execute_transfer(deps, env, msg, Amount::Native(coin), info.sender)
        // }
        ExecuteMsg::TransferToRemote(msg) => {
            let coin = one_coin(&info)?;
            let amount = Amount::from_parts(coin.denom, coin.amount);
            execute_transfer_back_to_remote_chain(deps, env, msg, amount, info.sender)
        }
        ExecuteMsg::UpdateMappingPair(msg) => execute_update_mapping_pair(deps, env, info, msg),
        ExecuteMsg::DeleteMappingPair(msg) => execute_delete_mapping_pair(deps, env, info, msg),
        // ExecuteMsg::Allow(allow) => execute_allow(deps, env, info, allow),
        ExecuteMsg::UpdateConfig {
            default_timeout,
            default_gas_limit,
            swap_router_contract,
            admin,
            token_fee,
            fee_receiver,
            relayer_fee_receiver,
            relayer_fee,
            converter_contract,
            osor_entrypoint_contract,
            token_factory_addr,
        } => update_config(
            deps,
            info,
            default_timeout,
            default_gas_limit,
            swap_router_contract,
            admin,
            token_fee,
            fee_receiver,
            relayer_fee_receiver,
            relayer_fee,
            converter_contract,
            osor_entrypoint_contract,
            token_factory_addr,
        ),
        // self-called msgs for ibc_packet_receive
        ExecuteMsg::IncreaseChannelBalanceIbcReceive {
            dest_channel_id,
            ibc_denom,
            amount,
            local_receiver,
        } => handle_increase_channel_balance_ibc_receive(
            deps,
            info.sender,
            env.contract.address,
            dest_channel_id,
            ibc_denom,
            amount,
            local_receiver,
        ),
        ExecuteMsg::ReduceChannelBalanceIbcReceive {
            src_channel_id,
            ibc_denom,
            amount,
            local_receiver,
        } => handle_reduce_channel_balance_ibc_receive(
            deps.storage,
            info.sender,
            env.contract.address,
            src_channel_id,
            ibc_denom,
            amount,
            local_receiver,
        ),
        ExecuteMsg::OverrideChannelBalance {
            channel_id,
            ibc_denom,
            outstanding,
            total_sent,
        } => handle_override_channel_balance(
            deps,
            info,
            channel_id,
            ibc_denom,
            outstanding,
            total_sent,
        ),
        ExecuteMsg::IbcHooksReceive {
            func,
            orai_receiver,
            args,
        } => ibc_hooks_receive(deps, env, info, func, orai_receiver, args),
        ExecuteMsg::RegisterDenom(msg) => register_denom(deps, env, info, msg),
        ExecuteMsg::WithdrawAsset { coin, receiver } => {
            execute_withdraw_asset(deps, info, coin, receiver)
        }
        // TODO: this msg for test only
        // need to remove after testing is done
        ExecuteMsg::ClockEndBlock { hash } => handle_clock_end_block_sudo(deps, hash)
    }
}

pub fn is_caller_contract(caller: Addr, contract_addr: Addr) -> StdResult<()> {
    if caller.ne(&contract_addr) {
        return Err(cosmwasm_std::StdError::generic_err(
            "Caller is not the contract itself!",
        ));
    }
    Ok(())
}

// withdraw stuck coin and transfer back to user
// only owner can execute
fn execute_withdraw_asset(
    deps: DepsMut,
    info: MessageInfo,
    coin: Amount,
    receiver: Option<Addr>,
) -> Result<Response, ContractError> {
    ADMIN.assert_admin(deps.as_ref(), &info.sender)?;
    let receiver = receiver.unwrap_or(info.sender);

    let msg = coin.send_amount(receiver.to_string(), None);

    Ok(Response::new()
        .add_attributes(vec![("action", "withdraw_asset")])
        .add_message(msg))
}

pub fn register_denom(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: RegisterDenomMsg,
) -> Result<Response, ContractError> {
    if !ADMIN.is_admin(deps.as_ref(), &info.sender)? && info.sender != env.contract.address {
        return Err(ContractError::Admin(AdminError::NotAdmin {}));
    };

    let config = CONFIG.load(deps.storage)?;

    let create_denom_msg = wasm_execute(
        config.token_factory_addr,
        &tokenfactory::msg::ExecuteMsg::CreateDenom {
            subdenom: msg.subdenom,
            metadata: msg.metadata,
        },
        info.funds,
    )?;

    Ok(Response::new()
        .add_attribute("action", "register_denom")
        .add_message(create_denom_msg))
}

pub fn handle_override_channel_balance(
    deps: DepsMut,
    info: MessageInfo,
    channel_id: String,
    ibc_denom: String,
    outstanding: Uint128,
    total_sent: Option<Uint128>,
) -> Result<Response, ContractError> {
    ADMIN.assert_admin(deps.as_ref(), &info.sender)?;
    override_channel_balance(
        deps.storage,
        &channel_id,
        &ibc_denom,
        outstanding,
        total_sent,
    )?;
    Ok(Response::new().add_attributes(vec![
        ("action", "override_channel_balance"),
        ("channel_id", &channel_id),
        ("ibc_denom", &ibc_denom),
        ("new_outstanding", &outstanding.to_string()),
        ("total_sent", &total_sent.unwrap_or_default().to_string()),
    ]))
}

pub fn handle_increase_channel_balance_ibc_receive(
    deps: DepsMut,
    caller: Addr,
    contract_addr: Addr,
    dst_channel_id: String,
    ibc_denom: String,
    remote_amount: Uint128,
    local_receiver: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    is_caller_contract(caller, contract_addr.clone())?;
    // will have to increase balance here because if this tx fails then it will be reverted, and the balance on the remote chain will also be reverted
    increase_channel_balance(deps.storage, &dst_channel_id, &ibc_denom, remote_amount)?;

    let mut cosmos_msgs: Vec<CosmosMsg> = vec![];
    let pair_mapping = ics20_denoms()
        .load(deps.storage, &ibc_denom)
        .map_err(|_| ContractError::NotOnMappingList {})?;

    let mint_amount = convert_remote_to_local(
        remote_amount,
        pair_mapping.remote_decimals,
        pair_mapping.asset_info_decimals,
    )?;
    let mint_msg = build_mint_mapping_msg(
        config.token_factory_addr.to_string(),
        pair_mapping.is_mint_burn,
        pair_mapping.asset_info,
        mint_amount,
        contract_addr.to_string(),
    )?;

    if let Some(mint_msg) = mint_msg {
        cosmos_msgs.push(mint_msg);
    }

    // we need to save the data to update the balances in reply
    let reply_args = ReplyArgs {
        channel: dst_channel_id.clone(),
        denom: ibc_denom.clone(),
        amount: remote_amount,
        local_receiver: local_receiver.clone(),
    };
    REPLY_ARGS.save(deps.storage, &reply_args)?;
    Ok(Response::default()
        .add_attributes(vec![
            ("action", "increase_channel_balance_ibc_receive"),
            ("channel_id", dst_channel_id.as_str()),
            ("ibc_denom", ibc_denom.as_str()),
            ("amount", remote_amount.to_string().as_str()),
            ("local_receiver", local_receiver.as_str()),
        ])
        .add_messages(cosmos_msgs))
}

pub fn handle_reduce_channel_balance_ibc_receive(
    storage: &mut dyn Storage,
    caller: Addr,
    contract_addr: Addr,
    src_channel_id: String,
    ibc_denom: String,
    remote_amount: Uint128,
    local_receiver: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(storage)?;
    is_caller_contract(caller, contract_addr.clone())?;
    // because we are transferring back, we reduce the channel's balance
    reduce_channel_balance(storage, src_channel_id.as_str(), &ibc_denom, remote_amount)
        .map_err(|err| StdError::generic_err(err.to_string()))?;

    // keep track of the single-step reply since we need ibc data to undo reducing channel balance and local data for refunding.
    // we use a different item to not override REPLY_ARGS
    SINGLE_STEP_REPLY_ARGS.save(
        storage,
        &ReplyArgs {
            channel: src_channel_id.to_string(),
            denom: ibc_denom.clone(),
            amount: remote_amount,
            local_receiver: local_receiver.to_string(),
        },
    )?;

    //  burn cw20 token if the mechanism is mint burn

    let mut cosmos_msgs: Vec<CosmosMsg> = vec![];
    let pair_mapping = ics20_denoms()
        .load(storage, &ibc_denom)
        .map_err(|_| ContractError::NotOnMappingList {})?;
    let burn_amount = convert_remote_to_local(
        remote_amount,
        pair_mapping.remote_decimals,
        pair_mapping.asset_info_decimals,
    )?;
    let burn_msg = build_burn_mapping_msg(
        config.token_factory_addr.to_string(),
        pair_mapping.is_mint_burn,
        pair_mapping.asset_info,
        burn_amount,
        contract_addr.to_string(),
    )?;
    if let Some(burn_msg) = burn_msg {
        cosmos_msgs.push(burn_msg);
    }

    Ok(Response::default()
        .add_attributes(vec![
            ("action", "reduce_channel_balance_ibc_receive"),
            ("channel_id", src_channel_id.as_str()),
            ("ibc_denom", ibc_denom.as_str()),
            ("amount", remote_amount.to_string().as_str()),
            ("local_receiver", local_receiver.as_str()),
        ])
        .add_messages(cosmos_msgs))
}

#[allow(clippy::too_many_arguments)]
pub fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    default_timeout: Option<u64>,
    default_gas_limit: Option<u64>,
    swap_router_contract: Option<String>,
    admin: Option<String>,
    token_fee: Option<Vec<TokenFee>>,
    fee_receiver: Option<String>,
    relayer_fee_receiver: Option<String>,
    relayer_fee: Option<Vec<RelayerFee>>,
    converter_contract: Option<String>,
    osor_entrypoint_contract: Option<String>,
    token_factory_addr: Option<String>,
) -> Result<Response, ContractError> {
    ADMIN.assert_admin(deps.as_ref(), &info.sender)?;
    if let Some(token_fee) = token_fee {
        for fee in token_fee {
            TOKEN_FEE.save(deps.storage, &fee.token_denom, &fee.ratio)?;
        }
    }
    if let Some(relayer_fee) = relayer_fee {
        for fee in relayer_fee {
            RELAYER_FEE.save(deps.storage, &fee.prefix, &fee.fee)?;
        }
    }
    CONFIG.update(deps.storage, |mut config| -> StdResult<Config> {
        if let Some(default_timeout) = default_timeout {
            config.default_timeout = default_timeout;
        }
        if let Some(swap_router_contract) = swap_router_contract {
            config.swap_router_contract = RouterController(swap_router_contract);
        }
        if let Some(fee_receiver) = fee_receiver {
            config.token_fee_receiver = deps.api.addr_validate(&fee_receiver)?;
        }
        if let Some(relayer_fee_receiver) = relayer_fee_receiver {
            config.relayer_fee_receiver = deps.api.addr_validate(&relayer_fee_receiver)?;
        }
        if let Some(converter_contract) = converter_contract {
            config.converter_contract = ConverterController(converter_contract);
        }
        if let Some(osor_entrypoint_contract) = osor_entrypoint_contract {
            config.osor_entrypoint_contract = osor_entrypoint_contract;
        }
        if let Some(token_factory_addr) = token_factory_addr {
            config.token_factory_addr = deps.api.addr_validate(&token_factory_addr)?;
        }
        config.default_gas_limit = default_gas_limit;
        Ok(config)
    })?;
    if let Some(admin) = admin {
        let admin = deps.api.addr_validate(&admin)?;
        ADMIN.execute_update_admin::<Empty, Empty>(deps, info, Some(admin))?;
    }
    Ok(Response::default().add_attribute("action", "update_config"))
}

pub fn execute_receive(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    wrapper: Cw20ReceiveMsg,
) -> Result<Response, ContractError> {
    nonpayable(&info)?;

    let amount = Amount::cw20(wrapper.amount, info.sender);
    let api = deps.api;

    let msg: TransferBackMsg = from_json(&wrapper.msg)?;
    execute_transfer_back_to_remote_chain(
        deps,
        env,
        msg,
        amount,
        api.addr_validate(&wrapper.sender)?,
    )
}

// pub fn execute_transfer(
//     deps: DepsMut,
//     env: Env,
//     msg: TransferMsg,
//     amount: Amount,
//     sender: Addr,
// ) -> Result<Response, ContractError> {
//     if amount.is_empty() {
//         return Err(ContractError::NoFunds {});
//     }
//     // ensure the requested channel is registered
//     if !CHANNEL_INFO.has(deps.storage, &msg.channel) {
//         return Err(ContractError::NoSuchChannel { id: msg.channel });
//     }
//     let config = CONFIG.load(deps.storage)?;

//     // if cw20 token, validate and ensure it is whitelisted, or we set default gas limit
//     if let Amount::Cw20(coin) = &amount {
//         let addr = deps.api.addr_validate(&coin.address)?;
//         // if limit is set, then we always allow cw20
//         if config.default_gas_limit.is_none() {
//             ALLOW_LIST
//                 .may_load(deps.storage, &addr)?
//                 .ok_or(ContractError::NotOnAllowList)?;
//         }
//     };

//     // delta from user is in seconds
//     let timeout_delta = match msg.timeout {
//         Some(t) => t,
//         None => config.default_timeout,
//     };
//     // timeout is in nanoseconds
//     let timeout = env.block.time.plus_seconds(timeout_delta);

//     // build ics20 packet
//     let packet = Ics20Packet::new(
//         amount.amount(),
//         amount.denom(),
//         sender.as_ref(),
//         &msg.remote_address,
//         msg.memo,
//     );
//     packet.validate()?;

//     // Update the balance now (optimistically) like ibctransfer modules.
//     // In on_packet_failure (ack with error message or a timeout), we reduce the balance appropriately.
//     // This means the channel works fine if success acks are not relayed.
//     increase_channel_balance(
//         deps.storage,
//         &msg.channel,
//         &amount.denom(),
//         amount.amount(),
//         true,
//     )?;

//     // prepare ibc message
//     let msg = IbcMsg::SendPacket {
//         channel_id: msg.channel,
//         data: to_json_binary(&packet)?,
//         timeout: timeout.into(),
//     };

//     // send response
//     let res = Response::new()
//         .add_message(msg)
//         .add_attribute("action", "transfer")
//         .add_attribute("sender", &packet.sender)
//         .add_attribute("receiver", &packet.receiver)
//         .add_attribute("denom", &packet.denom)
//         .add_attribute("amount", &packet.amount.to_string());
//     Ok(res)
// }

pub fn execute_transfer_back_to_remote_chain(
    deps: DepsMut,
    env: Env,
    msg: TransferBackMsg,
    amount: Amount,
    sender: Addr,
) -> Result<Response, ContractError> {
    if amount.is_empty() {
        return Err(ContractError::NoFunds {});
    }
    let config = CONFIG.load(deps.storage)?;

    // should be in form port/channel/denom
    let mappings =
        get_mappings_from_asset_info(deps.as_ref().storage, amount.into_asset_info(deps.api)?)?;

    // parse denom & compare with user input. Should not use string.includes() because hacker can fake a port that has the same remote denom to return true
    let mapping = mappings
        .into_iter()
        .find(|pair| -> bool {
            match parse_voucher_denom(
                pair.key.as_str(),
                &IbcEndpoint {
                    port_id: parse_ibc_wasm_port_id(env.contract.address.as_str()),
                    channel_id: msg.local_channel_id.clone(), // also verify local channel id
                },
            ) {
                Ok((denom, false)) => msg.remote_denom.eq(denom),
                _ => false,
            }
        })
        .ok_or(ContractError::MappingPairNotFound {})?;

    // if found mapping, then deduct fee based on mapping
    let fee_data = process_deduct_fee(
        deps.storage,
        &deps.querier,
        deps.api,
        &msg.remote_address,
        &msg.remote_denom,
        amount,
        &config.swap_router_contract,
    )?;

    let mut cosmos_msgs: Vec<CosmosMsg> = vec![];
    if !fee_data.token_fee.is_empty() {
        cosmos_msgs.push(
            fee_data
                .token_fee
                .send_amount(config.token_fee_receiver.into_string(), None),
        )
    }
    if !fee_data.relayer_fee.is_empty() {
        cosmos_msgs.push(
            fee_data
                .relayer_fee
                .send_amount(config.relayer_fee_receiver.into_string(), None),
        )
    }

    // send response
    let token_fee_str = fee_data.token_fee.amount().to_string();
    let relayer_fee_str = fee_data.relayer_fee.amount().to_string();
    let attributes = vec![
        ("action", "transfer_back_to_remote_chain"),
        ("sender", sender.as_str()),
        ("receiver", &msg.remote_address),
        ("token_fee", &token_fee_str),
        ("relayer_fee", &relayer_fee_str),
    ];

    // if our fees have drained the initial amount entirely, then we just get all the fees and that's it
    if fee_data.deducted_amount.is_zero() {
        return Ok(Response::new()
            .add_messages(cosmos_msgs)
            .add_attributes(attributes));
    }

    let ibc_denom = mapping.key;
    // ensure the requested channel is registered
    if !CHANNEL_INFO.has(deps.storage, &msg.local_channel_id) {
        return Err(ContractError::NoSuchChannel {
            id: msg.local_channel_id,
        });
    }

    // delta from user is in seconds
    let timeout = match msg.timeout {
        Some(t) => Timestamp::from_nanos(t),
        None => env.block.time.plus_seconds(config.default_timeout),
    };

    // need to convert decimal of cw20 to remote decimal before transferring
    let amount_remote = convert_local_to_remote(
        fee_data.deducted_amount,
        mapping.pair_mapping.remote_decimals,
        mapping.pair_mapping.asset_info_decimals,
    )?;

    // now this is processed in ack
    // // because we are transferring back, we reduce the channel's balance
    reduce_channel_balance(
        deps.storage,
        &msg.local_channel_id,
        &ibc_denom,
        amount_remote,
    )?;

    // prepare ibc message
    let ibc_msg = build_ibc_send_packet(
        amount_remote,
        &ibc_denom, // we use ibc denom in form <transfer>/<channel>/<denom> so that when it is sent back to remote chain, it gets parsed correctly and burned
        sender.as_str(),
        &msg.remote_address,
        msg.memo,
        &msg.local_channel_id,
        timeout.into(),
    )?;

    // build burn msg if the mechanism is mint/burn
    let burn_msg = build_burn_mapping_msg(
        config.token_factory_addr.to_string(),
        mapping.pair_mapping.is_mint_burn,
        mapping.pair_mapping.asset_info,
        fee_data.deducted_amount,
        env.contract.address.to_string(),
    )?;
    if let Some(burn_msg) = burn_msg {
        cosmos_msgs.push(burn_msg);
    }

    Ok(Response::new()
        .add_messages(cosmos_msgs)
        .add_message(ibc_msg)
        .add_attributes(attributes)
        .add_attributes(vec![
            ("denom", &ibc_denom),
            ("amount", &amount_remote.to_string()),
        ]))
}

pub fn build_burn_mapping_msg(
    token_factory: String,
    is_mint_burn: bool,
    burn_asset_info: AssetInfo,
    amount_local: Uint128,
    from_address: String,
) -> Result<Option<CosmosMsg>, ContractError> {
    //  burn cw20 token if the mechanism is mint burn
    if is_mint_burn {
        match burn_asset_info {
            AssetInfo::NativeToken { denom } => Ok(Some(
                wasm_execute(
                    token_factory,
                    &tokenfactory::msg::ExecuteMsg::BurnTokens {
                        denom: denom.to_owned(),
                        amount: amount_local,
                        burn_from_address: from_address,
                    },
                    vec![],
                )?
                .into(),
            )),
            AssetInfo::Token { contract_addr } => Ok(Some(
                wasm_execute(
                    contract_addr.to_string(),
                    &Cw20ExecuteMsg::Burn {
                        amount: amount_local,
                    },
                    vec![],
                )?
                .into(),
            )),
        }
    } else {
        Ok(None)
    }
}

pub fn build_mint_mapping_msg(
    token_factory: String,
    is_mint_burn: bool,
    mint_asset_info: AssetInfo,
    amount_local: Uint128,
    receiver: String,
) -> Result<Option<CosmosMsg>, ContractError> {
    if is_mint_burn {
        match mint_asset_info {
            AssetInfo::NativeToken { denom } => Ok(Some(
                wasm_execute(
                    token_factory,
                    &tokenfactory::msg::ExecuteMsg::MintTokens {
                        denom: denom.to_owned(),
                        amount: amount_local,
                        mint_to_address: receiver,
                    },
                    vec![],
                )?
                .into(),
            )),
            AssetInfo::Token { contract_addr } => Ok(Some(
                wasm_execute(
                    contract_addr.to_string(),
                    &Cw20ExecuteMsg::Mint {
                        recipient: receiver,
                        amount: amount_local,
                    },
                    vec![],
                )?
                .into(),
            )),
        }
    } else {
        Ok(None)
    }
}

// /// The gov contract can allow new contracts, or increase the gas limit on existing contracts.
// /// It cannot block or reduce the limit to avoid forcible sticking tokens in the channel.
// pub fn execute_allow(
//     deps: DepsMut,
//     _env: Env,
//     info: MessageInfo,
//     allow: AllowMsg,
// ) -> Result<Response, ContractError> {
//     ADMIN.assert_admin(deps.as_ref(), &info.sender)?;

//     let contract = deps.api.addr_validate(&allow.contract)?;
//     let set = AllowInfo {
//         gas_limit: allow.gas_limit,
//     };
//     ALLOW_LIST.update(deps.storage, &contract, |old| {
//         if let Some(old) = old {
//             // we must ensure it increases the limit
//             match (old.gas_limit, set.gas_limit) {
//                 (None, Some(_)) => return Err(ContractError::CannotLowerGas),
//                 (Some(old), Some(new)) if new < old => return Err(ContractError::CannotLowerGas),
//                 _ => {}
//             };
//         }
//         Ok(AllowInfo {
//             gas_limit: allow.gas_limit,
//         })
//     })?;

//     let gas = if let Some(gas) = allow.gas_limit {
//         gas.to_string()
//     } else {
//         "None".to_string()
//     };

//     let res = Response::new()
//         .add_attribute("action", "allow")
//         .add_attribute("contract", allow.contract)
//         .add_attribute("gas_limit", gas);
//     Ok(res)
// }

/// The gov contract can allow new contracts, or increase the gas limit on existing contracts.
/// It cannot block or reduce the limit to avoid forcible sticking tokens in the channel.
pub fn execute_update_mapping_pair(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mapping_pair_msg: UpdatePairMsg,
) -> Result<Response, ContractError> {
    ADMIN.assert_admin(deps.as_ref(), &info.sender)?;

    let ibc_denom = get_key_ics20_ibc_denom(
        &parse_ibc_wasm_port_id(env.contract.address.as_str()),
        &mapping_pair_msg.local_channel_id,
        &mapping_pair_msg.denom,
    );

    // if pair already exists in list, remove it and create a new one
    if ics20_denoms().load(deps.storage, &ibc_denom).is_ok() {
        ics20_denoms().remove(deps.storage, &ibc_denom)?;
    }

    ics20_denoms().save(
        deps.storage,
        &ibc_denom,
        &MappingMetadata {
            asset_info: mapping_pair_msg.local_asset_info.clone(),
            remote_decimals: mapping_pair_msg.remote_decimals,
            asset_info_decimals: mapping_pair_msg.local_asset_info_decimals,
            is_mint_burn: mapping_pair_msg.is_mint_burn.unwrap_or_default(),
        },
    )?;

    let res = Response::new()
        .add_attribute("action", "execute_update_mapping_pair")
        .add_attribute("denom", mapping_pair_msg.denom)
        .add_attribute(
            "new_asset_info",
            mapping_pair_msg.local_asset_info.to_string(),
        );
    Ok(res)
}

pub fn execute_delete_mapping_pair(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mapping_pair_msg: DeletePairMsg,
) -> Result<Response, ContractError> {
    ADMIN.assert_admin(deps.as_ref(), &info.sender)?;

    let ibc_denom = get_key_ics20_ibc_denom(
        &parse_ibc_wasm_port_id(env.contract.address.as_str()),
        &mapping_pair_msg.local_channel_id,
        &mapping_pair_msg.denom,
    );

    ics20_denoms().remove(deps.storage, &ibc_denom)?;

    let res = Response::new()
        .add_attribute("action", "execute_delete_mapping_pair")
        .add_attribute("local_channel_id", mapping_pair_msg.local_channel_id)
        .add_attribute("original_denom", mapping_pair_msg.denom);
    Ok(res)
}

#[entry_point]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    // we don't need to save anything if migrating from the same version
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new())
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Port {} => to_json_binary(&query_port(deps)?),
        QueryMsg::ListChannels {} => to_json_binary(&query_list(deps)?),
        QueryMsg::Channel { id } => to_json_binary(&query_channel(deps, id)?),
        QueryMsg::ChannelWithKey { channel_id, denom } => {
            to_json_binary(&query_channel_with_key(deps, channel_id, denom)?)
        }
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::Allowed { contract } => to_json_binary(&query_allowed(deps, contract)?),
        QueryMsg::ListAllowed {
            start_after,
            limit,
            order,
        } => to_json_binary(&list_allowed(deps, start_after, limit, order)?),
        QueryMsg::PairMappings {
            start_after,
            limit,
            order,
        } => to_json_binary(&list_cw20_mapping(deps, start_after, limit, order)?),
        QueryMsg::PairMapping { key } => to_json_binary(&get_mapping_from_key(deps, key)?),
        QueryMsg::PairMappingsFromAssetInfo { asset_info } => {
            to_json_binary(&get_mappings_from_asset_info(deps.storage, asset_info)?)
        }
        QueryMsg::Admin {} => to_json_binary(&ADMIN.query_admin(deps)?),
        QueryMsg::GetTransferTokenFee { remote_token_denom } => {
            to_json_binary(&TOKEN_FEE.load(deps.storage, &remote_token_denom)?)
        }
        QueryMsg::RefundInfoList {} => to_json_binary(&REFUND_INFO_LIST.load(deps.storage)?)
    }
}

fn query_port(deps: Deps) -> StdResult<PortResponse> {
    let query = IbcQuery::PortId {}.into();
    let PortIdResponse { port_id } = deps.querier.query(&query)?;
    Ok(PortResponse { port_id })
}

fn query_list(deps: Deps) -> StdResult<ListChannelsResponse> {
    let channels = CHANNEL_INFO
        .range_raw(deps.storage, None, None, Order::Ascending)
        .map(|r| r.map(|(_, v)| v))
        .collect::<StdResult<_>>()?;
    Ok(ListChannelsResponse { channels })
}

// make public for ibc tests
pub fn query_channel(deps: Deps, id: String) -> StdResult<ChannelResponse> {
    let info = CHANNEL_INFO.load(deps.storage, &id)?;
    // this returns Vec<(outstanding, total)>
    let channel_state = CHANNEL_REVERSE_STATE;
    let state = channel_state
        .prefix(&id)
        .range(deps.storage, None, None, Order::Ascending)
        .map(|r| {
            // this denom is
            r.map(|(denom, v)| {
                let outstanding = Amount::from_parts(denom.clone(), v.outstanding);
                let total = Amount::from_parts(denom, v.total_sent);
                (outstanding, total)
            })
        })
        .collect::<StdResult<Vec<_>>>()?;
    // we want (Vec<outstanding>, Vec<total>)
    let (balances, total_sent): (Vec<Amount>, Vec<Amount>) = state.into_iter().unzip();

    Ok(ChannelResponse {
        info,
        balances,
        total_sent,
    })
}

pub fn query_channel_with_key(
    deps: Deps,
    channel_id: String,
    denom: String,
) -> StdResult<ChannelWithKeyResponse> {
    let info = CHANNEL_INFO.load(deps.storage, &channel_id)?;
    // this returns Vec<(outstanding, total)>
    let (balance, total_sent) = CHANNEL_REVERSE_STATE
        .load(deps.storage, (&channel_id, &denom))
        .map(|channel_state| {
            let outstanding = Amount::from_parts(denom.clone(), channel_state.outstanding);
            let total = Amount::from_parts(denom, channel_state.total_sent);
            (outstanding, total)
        })?;

    Ok(ChannelWithKeyResponse {
        info,
        balance,
        total_sent,
    })
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let cfg = CONFIG.load(deps.storage)?;
    let admin = ADMIN.get(deps)?.unwrap_or_else(|| Addr::unchecked(""));
    let res = ConfigResponse {
        default_timeout: cfg.default_timeout,
        default_gas_limit: cfg.default_gas_limit,
        fee_denom: cfg.fee_denom,
        swap_router_contract: cfg.swap_router_contract.addr(),
        gov_contract: admin.into(),
        relayer_fee_receiver: cfg.relayer_fee_receiver,
        token_fee_receiver: cfg.token_fee_receiver,
        token_fees: TOKEN_FEE
            .range(deps.storage, None, None, Order::Ascending)
            .map(|data_result| {
                let (token_denom, ratio) = data_result?;
                Ok(TokenFee { token_denom, ratio })
            })
            .collect::<StdResult<_>>()?,
        relayer_fees: RELAYER_FEE
            .range(deps.storage, None, None, Order::Ascending)
            .map(|data_result| {
                let (prefix, amount) = data_result?;
                Ok(RelayerFeeResponse { prefix, amount })
            })
            .collect::<StdResult<_>>()?,
        converter_contract: cfg.converter_contract.addr(),
        osor_entrypoint_contract: cfg.osor_entrypoint_contract,
    };
    Ok(res)
}

fn query_allowed(deps: Deps, contract: String) -> StdResult<AllowedResponse> {
    let addr = deps.api.addr_validate(&contract)?;
    let info = ALLOW_LIST.may_load(deps.storage, &addr)?;
    let res = match info {
        None => AllowedResponse {
            is_allowed: false,
            gas_limit: None,
        },
        Some(a) => AllowedResponse {
            is_allowed: true,
            gas_limit: a.gas_limit,
        },
    };
    Ok(res)
}

// settings for pagination
const MAX_LIMIT: u32 = 30;
const DEFAULT_LIMIT: u32 = 10;

fn list_allowed(
    deps: Deps,
    start_after: Option<String>,
    limit: Option<u32>,
    order: Option<u8>,
) -> StdResult<ListAllowedResponse> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let addr = maybe_addr(deps.api, start_after)?;
    let start = addr.as_ref().map(Bound::exclusive);

    let allow = ALLOW_LIST
        .range(deps.storage, start, None, map_order(order))
        .take(limit)
        .map(|item| {
            item.map(|(addr, allow)| AllowedInfo {
                contract: addr.into(),
                gas_limit: allow.gas_limit,
            })
        })
        .collect::<StdResult<_>>()?;
    Ok(ListAllowedResponse { allow })
}

fn list_cw20_mapping(
    deps: Deps,
    start_after: Option<String>,
    limit: Option<u32>,
    order: Option<u8>,
) -> StdResult<ListMappingResponse> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let mut allow_range = ics20_denoms().range(deps.storage, None, None, map_order(order));
    if let Some(start_after) = start_after {
        let start = Some(Bound::exclusive::<&str>(&start_after));
        allow_range = ics20_denoms().range(deps.storage, start, None, map_order(order));
    }
    let pairs = allow_range
        .take(limit)
        .map(|item| {
            item.map(|(key, mapping)| PairQuery {
                key,
                pair_mapping: mapping,
            })
        })
        .collect::<StdResult<_>>()?;
    Ok(ListMappingResponse { pairs })
}

fn get_mapping_from_key(deps: Deps, ibc_denom: String) -> StdResult<PairQuery> {
    let result = ics20_denoms().load(deps.storage, &ibc_denom)?;
    Ok(PairQuery {
        key: ibc_denom,
        pair_mapping: result,
    })
}

fn map_order(order: Option<u8>) -> Order {
    match order {
        Some(order) => {
            if order == 1 {
                Order::Ascending
            } else {
                Order::Descending
            }
        }
        None => Order::Ascending,
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(deps: DepsMut, _env: Env, msg: SudoMsg) -> Result<Response, ContractError> {
    match msg {
        SudoMsg::ClockEndBlock { hash } => handle_clock_end_block_sudo(deps, hash)
    }
}

pub fn handle_clock_end_block_sudo(deps: DepsMut, _hash: String) -> Result<Response, ContractError> {
    let mut cosmos_msgs: Vec<CosmosMsg> = vec![];

    // get list of refund info
    let refund_info_list = REFUND_INFO_LIST.load(deps.storage)?;

    // if refund list is empty, return response
    if refund_info_list.len() == 0 {
        let response = Response::new().add_attribute("action", "auto_refund");

        return Ok(response);
    }

    for refund_info in refund_info_list.into_iter() {
        let msg = refund_info
            .amount
            .send_amount(refund_info.clone().receiver, None);
        cosmos_msgs.push(msg);
    }

    // clear refund lists
    REFUND_INFO_LIST.update(deps.storage, |mut lists| -> StdResult<_> {
        lists.clear();
        StdResult::Ok(lists)
    })?;

    let response = Response::new()
        .add_messages(cosmos_msgs)
        .add_attribute("action", "auto_refund");

    Ok(response)
}