use std::ops::Sub;

use cosmwasm_std::{
    wasm_execute, Addr, BankMsg, Coin, CosmosMsg, Decimal, IbcChannelConnectMsg, IbcChannelOpenMsg,
    StdError,
};
use cosmwasm_testing_util::mock::MockContract;
use cosmwasm_vm::testing::MockInstanceOptions;
use cw20_ics20_msg::converter::ConverterController;
use cw20_ics20_msg::helper::get_full_denom;
use cw_controllers::AdminError;
use oraiswap::asset::AssetInfo;
use oraiswap::router::RouterController;

use crate::ibc::{
    convert_remote_denom_to_evm_prefix, deduct_fee, deduct_relayer_fee, deduct_token_fee,
    get_follow_up_msgs, get_swap_token_amount_out_from_orai, handle_packet_refund,
    ibc_packet_receive, parse_ibc_channel_without_sanity_checks,
    parse_ibc_denom_without_sanity_checks, parse_ibc_info_without_sanity_checks,
    parse_voucher_denom, Ics20Ack, Ics20Packet, ICS20_VERSION, NATIVE_RECEIVE_ID,
    REFUND_FAILURE_ID,
};
use crate::query_helper::get_destination_info_on_orai;
use crate::testing::test_helpers::*;
use cosmwasm_std::{
    from_json, to_json_binary, IbcEndpoint, IbcMsg, IbcPacket, IbcPacketReceiveMsg, SubMsg,
    Timestamp, Uint128, WasmMsg,
};

use crate::error::ContractError;
use crate::state::{
    get_key_ics20_ibc_denom, ics20_denoms, increase_channel_balance, reduce_channel_balance,
    Config, ADMIN, CHANNEL_REVERSE_STATE, CONFIG, RELAYER_FEE, REPLY_ARGS, TOKEN_FEE,
};
use cw20::{Cw20CoinVerified, Cw20ExecuteMsg, Cw20ReceiveMsg};
use cw20_ics20_msg::amount::{convert_remote_to_local, Amount};
use cw20_ics20_msg::state::{MappingMetadata, Ratio, RelayerFee, TokenFee};

use crate::contract::{
    build_burn_mapping_msg, build_mint_mapping_msg, execute, handle_override_channel_balance,
    query, query_channel, query_channel_with_key,
};
use crate::msg::{
    AllowMsg, ChannelResponse, ConfigResponse, ExecuteMsg, InitMsg, ListChannelsResponse,
    ListMappingResponse, PairQuery, QueryMsg, RegisterDenomMsg,
};
use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{coins, to_json_vec};
use cw20_ics20_msg::msg::{DeletePairMsg, TransferBackMsg, UpdatePairMsg};

const SENDER: &str = "orai1gkr56hlnx9vc7vncln2dkd896zfsqjn300kfq0";
const CONTRACT: &str = "orai19p43y0tqnr5qlhfwnxft2u5unph5yn60y7tuvu";

#[test]
fn check_ack_json() {
    let success = Ics20Ack::Result(b"1".into());
    let fail = Ics20Ack::Error("bad coin".into());

    let success_json = String::from_utf8(to_json_vec(&success).unwrap()).unwrap();
    assert_eq!(r#"{"result":"MQ=="}"#, success_json.as_str());

    let fail_json = String::from_utf8(to_json_vec(&fail).unwrap()).unwrap();
    assert_eq!(r#"{"error":"bad coin"}"#, fail_json.as_str());
}

#[test]
fn test_sub_negative() {
    assert_eq!(
        Uint128::from(10u128)
            .checked_sub(Uint128::from(11u128))
            .unwrap_or_default(),
        Uint128::from(0u128)
    )
}

#[test]
fn check_packet_json() {
    let packet = Ics20Packet::new(
        Uint128::new(12345),
        "ucosm",
        "cosmos1zedxv25ah8fksmg2lzrndrpkvsjqgk4zt5ff7n",
        "wasm1fucynrfkrt684pm8jrt8la5h2csvs5cnldcgqc",
        None,
    );
    // Example message generated from the SDK
    let expected = r#"{"amount":"12345","denom":"ucosm","receiver":"wasm1fucynrfkrt684pm8jrt8la5h2csvs5cnldcgqc","sender":"cosmos1zedxv25ah8fksmg2lzrndrpkvsjqgk4zt5ff7n","memo":null}"#;

    let encdoded = String::from_utf8(to_json_vec(&packet).unwrap()).unwrap();
    assert_eq!(expected, encdoded.as_str());
}

// #[test]
// fn check_gas_limit_handles_all_cases() {
//     let send_channel = "channel-9";
//     let allowed = "foobar";
//     let allowed_gas = 777666;
//     let mut deps = setup(&[send_channel], &[(allowed, allowed_gas)]);

//     // allow list will get proper gas
//     let limit = check_gas_limit(deps.as_ref(), &Amount::cw20(500, allowed)).unwrap();
//     assert_eq!(limit, Some(allowed_gas));

//     // non-allow list will error
//     let random = "tokenz";
//     check_gas_limit(deps.as_ref(), &Amount::cw20(500, random)).unwrap_err();

//     // add default_gas_limit
//     let def_limit = 54321;
//     migrate(
//         deps.as_mut(),
//         mock_env(),
//         MigrateMsg {
//             // default_gas_limit: Some(def_limit),
//             // token_fee_receiver: "receiver".to_string(),
//             // relayer_fee_receiver: "relayer_fee_receiver".to_string(),
//             // default_timeout: 100u64,
//             // fee_denom: "orai".to_string(),
//             // swap_router_contract: "foobar".to_string(),
//         },
//     )
//     .unwrap();

//     // allow list still gets proper gas
//     let limit = check_gas_limit(deps.as_ref(), &Amount::cw20(500, allowed)).unwrap();
//     assert_eq!(limit, Some(allowed_gas));

//     // non-allow list will now get default
//     let limit = check_gas_limit(deps.as_ref(), &Amount::cw20(500, random)).unwrap();
//     assert_eq!(limit, Some(def_limit));
// }

// test remote chain send native token to local chain
fn mock_receive_packet_remote_to_local(
    my_channel: &str,
    amount: u128,
    denom: &str,
    receiver: &str,
    sender: Option<&str>,
) -> IbcPacket {
    let data = Ics20Packet {
        // this is returning a foreign native token, thus denom is <denom>, eg: uatom
        denom: denom.to_string(),
        amount: amount.into(),
        sender: if sender.is_none() {
            "remote-sender".to_string()
        } else {
            sender.unwrap().to_string()
        },
        receiver: receiver.to_string(),
        memo: None,
    };
    IbcPacket::new(
        to_json_binary(&data).unwrap(),
        IbcEndpoint {
            port_id: REMOTE_PORT.to_string(),
            channel_id: "channel-1234".to_string(),
        },
        IbcEndpoint {
            port_id: CONTRACT_PORT.to_string(),
            channel_id: my_channel.to_string(),
        },
        3,
        Timestamp::from_seconds(1665321069).into(),
    )
}

#[test]
fn test_parse_voucher_denom_invalid_length() {
    let voucher_denom = "foobar/foobar";
    let ibc_endpoint = IbcEndpoint {
        port_id: "hello".to_string(),
        channel_id: "world".to_string(),
    };
    // native denom case
    assert_eq!(
        parse_voucher_denom(voucher_denom, &ibc_endpoint).unwrap_err(),
        ContractError::NoForeignTokens {}
    );
}

#[test]
fn test_parse_voucher_denom_invalid_port() {
    let voucher_denom = "foobar/abc/xyz";
    let ibc_endpoint = IbcEndpoint {
        port_id: "hello".to_string(),
        channel_id: "world".to_string(),
    };
    // native denom case
    assert_eq!(
        parse_voucher_denom(voucher_denom, &ibc_endpoint).unwrap_err(),
        ContractError::FromOtherPort {
            port: "foobar".to_string()
        }
    );
}

#[test]
fn test_parse_voucher_denom_invalid_channel() {
    let voucher_denom = "hello/abc/xyz";
    let ibc_endpoint = IbcEndpoint {
        port_id: "hello".to_string(),
        channel_id: "world".to_string(),
    };
    // native denom case
    assert_eq!(
        parse_voucher_denom(voucher_denom, &ibc_endpoint).unwrap_err(),
        ContractError::FromOtherChannel {
            channel: "abc".to_string()
        }
    );
}

#[test]
fn test_parse_voucher_denom_native_denom_valid() {
    let voucher_denom = "foobar";
    let ibc_endpoint = IbcEndpoint {
        port_id: "hello".to_string(),
        channel_id: "world".to_string(),
    };
    assert_eq!(
        parse_voucher_denom(voucher_denom, &ibc_endpoint).unwrap(),
        ("foobar", true)
    );
}

/////////////////////////////// Test cases for native denom transfer from remote chain to local chain

#[test]
fn send_native_from_remote_mapping_not_found() {
    let relayer = Addr::unchecked("relayer");
    let send_channel = "channel-9";
    let cw20_addr = "token-addr";
    let custom_addr = "custom-addr";
    let cw20_denom = "cw20:token-addr";
    let gas_limit = 1234567;
    let mut deps = setup(
        &["channel-1", "channel-7", send_channel],
        &[(cw20_addr, gas_limit)],
    );
    let config = CONFIG.load(deps.as_ref().storage).unwrap();
    // prepare some mock packets
    let recv_packet =
        mock_receive_packet_remote_to_local(send_channel, 876543210, cw20_denom, custom_addr, None);

    // we can receive this denom, channel balance should increase
    let msg = IbcPacketReceiveMsg::new(recv_packet.clone(), relayer);
    let res = ibc_packet_receive(deps.as_mut(), mock_env(), msg).unwrap();
    println!("res.messages: {:?}", res.messages[0].msg);
    assert_eq!(
        res.messages[0].msg,
        wasm_execute(
            "cosmos2contract",
            &ExecuteMsg::RegisterDenom(RegisterDenomMsg {
                subdenom: String::from("cw20:token-addr"),
                metadata: None
            }),
            vec![Coin::new(1u128.into(), "orai")]
        )
        .unwrap()
        .into()
    );
    execute(
        deps.as_mut(),
        mock_env(),
        mock_info("cosmos2contract", &[]),
        ExecuteMsg::RegisterDenom(RegisterDenomMsg {
            subdenom: String::from("cw20:token-addr"),
            metadata: None,
        }),
    )
    .unwrap();
    let pair_mapping = ics20_denoms()
        .load(
            deps.as_ref().storage,
            "wasm.cosmos2contract/channel-9/cw20:token-addr",
        )
        .unwrap();
    assert_eq!(
        pair_mapping,
        MappingMetadata {
            asset_info: AssetInfo::NativeToken {
                denom: get_full_denom(config.token_factory_addr.to_string(), cw20_denom.into()),
            },
            remote_decimals: 1,
            asset_info_decimals: 1,
            is_mint_burn: true
        }
    );
}

#[test]
fn proper_checks_on_execute_native_transfer_back_to_remote() {
    // arrange
    let relayer = Addr::unchecked("relayer");
    let remote_channel = "channel-5";
    let remote_address = "cosmos1603j3e4juddh7cuhfquxspl0p0nsun046us7n0";
    let custom_addr = "custom-addr";
    let original_sender = "original_sender";
    let denom = "uatom0x";
    let amount = Uint128::from(1234567u128);
    let token_addr = Addr::unchecked("token-addr".to_string());
    let asset_info = AssetInfo::Token {
        contract_addr: token_addr.clone(),
    };
    let cw20_raw_denom = token_addr.as_str();
    let local_channel = "channel-1234";
    let ibc_denom = get_key_ics20_ibc_denom("wasm.cosmos2contract", local_channel, denom);
    let ratio = Ratio {
        nominator: 1,
        denominator: 10,
    };
    let fee_amount = amount * Decimal::from_ratio(ratio.nominator, ratio.denominator);
    let mut deps = setup(&[remote_channel, local_channel], &[]);
    TOKEN_FEE
        .save(deps.as_mut().storage, denom, &ratio)
        .unwrap();

    let pair = UpdatePairMsg {
        local_channel_id: local_channel.to_string(),
        denom: denom.to_string(),
        local_asset_info: asset_info.clone(),
        remote_decimals: 18u8,
        local_asset_info_decimals: 18u8,
        is_mint_burn: None,
    };

    let _ = execute(
        deps.as_mut(),
        mock_env(),
        mock_info("gov", &[]),
        ExecuteMsg::UpdateMappingPair(pair),
    )
    .unwrap();

    // execute
    let mut transfer = TransferBackMsg {
        local_channel_id: local_channel.to_string(),
        remote_address: remote_address.to_string(),
        remote_denom: denom.to_string(),
        timeout: Some(DEFAULT_TIMEOUT),
        memo: None,
    };

    let msg = ExecuteMsg::Receive(Cw20ReceiveMsg {
        sender: original_sender.to_string(),
        amount,
        msg: to_json_binary(&transfer).unwrap(),
    });

    // insufficient funds case because we need to receive from remote chain first
    let info = mock_info(cw20_raw_denom, &[]);
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg.clone());
    assert_eq!(
        res.unwrap_err(),
        ContractError::NoSuchChannelState {
            id: local_channel.to_string(),
            denom: get_key_ics20_ibc_denom("wasm.cosmos2contract", local_channel, denom)
        }
    );

    // prepare some mock packets
    let recv_packet = mock_receive_packet(
        remote_channel,
        local_channel,
        amount,
        denom.to_string(),
        custom_addr.to_string(),
    );

    // receive some tokens. Assume that the function works perfectly because the test case is elsewhere
    let ibc_msg = IbcPacketReceiveMsg::new(recv_packet.clone(), relayer);
    ibc_packet_receive(deps.as_mut(), mock_env(), ibc_msg).unwrap();
    // need to trigger increase channel balance because it is executed through submsg
    execute(
        deps.as_mut(),
        mock_env(),
        mock_info(mock_env().contract.address.as_str(), &[]),
        ExecuteMsg::IncreaseChannelBalanceIbcReceive {
            dest_channel_id: local_channel.to_string(),
            ibc_denom: ibc_denom.clone(),
            amount: Uint128::from(amount),
            local_receiver: custom_addr.to_string(),
        },
    )
    .unwrap();

    // error cases
    // revert transfer state to correct state
    transfer.local_channel_id = local_channel.to_string();
    let receive_msg = ExecuteMsg::Receive(Cw20ReceiveMsg {
        sender: original_sender.to_string(),
        amount: Uint128::from(amount),
        msg: to_json_binary(&transfer).unwrap(),
    });

    // now we execute transfer back to remote chain
    let res = execute(deps.as_mut(), mock_env(), info.clone(), receive_msg).unwrap();

    assert_eq!(res.messages[0].gas_limit, None);
    println!("res messages: {:?}", res.messages);
    assert_eq!(res.messages.len(), 2); // 2 because it also has deduct fee msg
    match res.messages[1].msg.clone() {
        CosmosMsg::Ibc(IbcMsg::SendPacket {
            channel_id,
            data,
            timeout,
        }) => {
            let expected_timeout = DEFAULT_TIMEOUT;
            assert_eq!(timeout.timestamp().unwrap().nanos(), expected_timeout);
            assert_eq!(channel_id.as_str(), local_channel);
            let msg: Ics20Packet = from_json(&data).unwrap();
            assert_eq!(
                msg.amount,
                Uint128::new(1234567).sub(Uint128::from(fee_amount))
            );
            assert_eq!(
                msg.denom.as_str(),
                get_key_ics20_ibc_denom(CONTRACT_PORT, local_channel, denom)
            );
            assert_eq!(msg.sender.as_str(), original_sender);
            assert_eq!(msg.receiver.as_str(), remote_address);
            // assert_eq!(msg.memo, None);
        }
        _ => panic!("Unexpected return message: {:?}", res.messages[0]),
    }
    match res.messages[0].msg.clone() {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds: _,
        }) => {
            assert_eq!(contract_addr, token_addr.to_string());
            assert_eq!(
                msg,
                to_json_binary(&Cw20ExecuteMsg::Transfer {
                    recipient: "gov".to_string(),
                    amount: fee_amount
                })
                .unwrap()
            );
        }
        _ => panic!("Unexpected return message: {:?}", res.messages[0]),
    }

    // check new channel state after reducing balance
    let chan = query_channel(deps.as_ref(), local_channel.into()).unwrap();
    assert_eq!(
        chan.balances,
        vec![Amount::native(
            fee_amount,
            get_key_ics20_ibc_denom(CONTRACT_PORT, local_channel, denom)
        )]
    );
    assert_eq!(
        chan.total_sent,
        vec![Amount::native(
            amount,
            get_key_ics20_ibc_denom(CONTRACT_PORT, local_channel, denom)
        )]
    );

    // mapping pair error with wrong voucher denom
    let pair = UpdatePairMsg {
        local_channel_id: "not_registered_channel".to_string(),
        denom: denom.to_string(),
        local_asset_info: AssetInfo::Token {
            contract_addr: Addr::unchecked("random_cw20_denom".to_string()),
        },
        remote_decimals: 18u8,
        local_asset_info_decimals: 18u8,
        is_mint_burn: None,
    };

    execute(
        deps.as_mut(),
        mock_env(),
        mock_info("gov", &[]),
        ExecuteMsg::UpdateMappingPair(pair),
    )
    .unwrap();

    transfer.local_channel_id = "not_registered_channel".to_string();
    let invalid_msg = ExecuteMsg::Receive(Cw20ReceiveMsg {
        sender: original_sender.to_string(),
        amount: Uint128::from(amount),
        msg: to_json_binary(&transfer).unwrap(),
    });
    let err = execute(deps.as_mut(), mock_env(), info.clone(), invalid_msg).unwrap_err();
    assert_eq!(err, ContractError::MappingPairNotFound {});
}

#[test]
fn send_from_remote_to_local_receive_happy_path() {
    let mut contract_instance = MockContract::new(
        WASM_BYTES,
        Addr::unchecked(CONTRACT),
        MockInstanceOptions {
            balances: &[(SENDER, &coins(100_000_000_000, "orai"))],
            gas_limit: 40_000_000_000_000_000,
            ..MockInstanceOptions::default()
        },
    );
    let cw20_addr = "orai1lus0f0rhx8s03gdllx2n6vhkmf0536dv57wfge";
    let relayer = Addr::unchecked("orai12zyu8w93h0q2lcnt50g3fn0w3yqnhy4fvawaqz");
    let send_channel = "channel-9";
    let custom_addr = "orai12zyu8w93h0q2lcnt50g3fn0w3yqnhy4fvawaqz";
    let denom = "uatom0x";
    let asset_info = AssetInfo::Token {
        contract_addr: Addr::unchecked(cw20_addr),
    };
    let contract_port = format!("wasm.{}", CONTRACT);
    let gas_limit = 1234567;
    let send_amount = Uint128::from(876543210u64);
    let channels = &["channel-1", "channel-7", send_channel];

    let allow = &[(cw20_addr, gas_limit)];

    let allowlist = allow
        .iter()
        .map(|(contract, gas)| AllowMsg {
            contract: contract.to_string(),
            gas_limit: Some(*gas),
        })
        .collect();

    // instantiate an empty contract
    let instantiate_msg = InitMsg {
        default_gas_limit: None,
        default_timeout: DEFAULT_TIMEOUT,
        gov_contract: SENDER.to_string(),
        allowlist,
        swap_router_contract: "router".to_string(),
        converter_contract: "converter".to_string(),
        osor_entrypoint_contract: "osor_entrypoint_contract".to_string(),
        token_factory_addr: "orai17hyr3eg92fv34fdnkend48scu32hn26gqxw3hnwkfy904lk9r09qqzty42"
            .to_string(),
    };

    contract_instance
        .instantiate(instantiate_msg, SENDER, &[])
        .unwrap();

    for channel_id in channels {
        let channel = mock_channel(channel_id);
        let open_msg = IbcChannelOpenMsg::new_init(channel.clone());
        contract_instance.ibc_channel_open(open_msg).unwrap();
        let connect_msg = IbcChannelConnectMsg::new_ack(channel, ICS20_VERSION);
        contract_instance.ibc_channel_connect(connect_msg).unwrap();
    }

    contract_instance
        .with_storage(|storage| {
            TOKEN_FEE
                .save(
                    storage,
                    denom,
                    &Ratio {
                        nominator: 1,
                        denominator: 10,
                    },
                )
                .unwrap();
            Ok(())
        })
        .unwrap();

    let pair = UpdatePairMsg {
        local_channel_id: send_channel.to_string(),
        denom: denom.to_string(),
        local_asset_info: asset_info.clone(),
        remote_decimals: 18u8,
        local_asset_info_decimals: 18u8,
        is_mint_burn: None,
    };

    contract_instance
        .execute(ExecuteMsg::UpdateMappingPair(pair), SENDER, &[])
        .unwrap();

    let data = Ics20Packet {
        // this is returning a foreign native token, thus denom is <denom>, eg: uatom
        denom: denom.to_string(),
        amount: send_amount,
        sender: SENDER.to_string(),
        receiver: custom_addr.to_string(),
        memo: None,
    };
    let recv_packet = IbcPacket::new(
        to_json_binary(&data).unwrap(),
        IbcEndpoint {
            port_id: REMOTE_PORT.to_string(),
            channel_id: "channel-1234".to_string(),
        },
        IbcEndpoint {
            port_id: contract_port.clone(),
            channel_id: send_channel.to_string(),
        },
        3,
        Timestamp::from_seconds(1665321069).into(),
    );

    // we can receive this denom, channel balance should increase
    let ibc_msg = IbcPacketReceiveMsg::new(recv_packet.clone(), relayer);

    let (res, _gas_used) = contract_instance.ibc_packet_receive(ibc_msg).unwrap();

    // TODO: fix test cases. Possibly because we are adding two add_submessages?
    assert_eq!(res.messages.len(), 3); // 3 messages because we also have deduct fee msg and increase channel balance msg
    match res.messages[1].msg.clone() {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds: _,
        }) => {
            assert_eq!(contract_addr, cw20_addr);
            assert_eq!(
                msg,
                to_json_binary(&Cw20ExecuteMsg::Transfer {
                    recipient: SENDER.to_string(),
                    amount: Uint128::from(87654321u64) // send amount / token fee
                })
                .unwrap()
            );
        }
        _ => panic!("Unexpected return message: {:?}", res.messages[0]),
    }

    let ack: Ics20Ack = from_json(&res.acknowledgement).unwrap();
    assert!(matches!(ack, Ics20Ack::Result(_)));

    // query channel state|_|
    match res.messages[0].msg.clone() {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds: _,
        }) => {
            assert_eq!(contract_addr, CONTRACT); // self-call msg
            assert_eq!(
                msg,
                to_json_binary(&ExecuteMsg::IncreaseChannelBalanceIbcReceive {
                    dest_channel_id: send_channel.to_string(),
                    ibc_denom: get_key_ics20_ibc_denom(contract_port.as_str(), send_channel, denom),
                    amount: send_amount,
                    local_receiver: custom_addr.to_string(),
                })
                .unwrap()
            );
        }
        _ => panic!("Unexpected return message: {:?}", res.messages[0]),
    }
}

#[test]
fn test_follow_up_msgs() {}

#[test]
fn test_deduct_fee() {
    assert_eq!(
        deduct_fee(
            Ratio {
                nominator: 1,
                denominator: 0,
            },
            Uint128::from(1000u64)
        ),
        Uint128::zero()
    );
    assert_eq!(
        deduct_fee(
            Ratio {
                nominator: 1,
                denominator: 1,
            },
            Uint128::from(1000u64)
        ),
        Uint128::from(1000u64)
    );
    assert_eq!(
        deduct_fee(
            Ratio {
                nominator: 1,
                denominator: 100,
            },
            Uint128::from(1000u64)
        ),
        Uint128::from(10u64)
    );
}

#[test]
fn test_convert_remote_denom_to_evm_prefix() {
    assert_eq!(convert_remote_denom_to_evm_prefix("abcd"), "".to_string());
    assert_eq!(convert_remote_denom_to_evm_prefix("0x"), "".to_string());
    assert_eq!(
        convert_remote_denom_to_evm_prefix("evm0x"),
        "evm".to_string()
    );
}

#[test]
fn test_parse_ibc_denom_without_sanity_checks() {
    assert_eq!(parse_ibc_denom_without_sanity_checks("foo").is_err(), true);
    assert_eq!(
        parse_ibc_denom_without_sanity_checks("foo/bar").is_err(),
        true
    );
    let result = parse_ibc_denom_without_sanity_checks("foo/bar/helloworld").unwrap();
    assert_eq!(result, "helloworld");

    let result = parse_ibc_info_without_sanity_checks("foo/bar").unwrap_or_default();
    assert_eq!(result.0, "");
    assert_eq!(result.1, "");
    assert_eq!(result.2, "");
}

#[test]
fn test_parse_ibc_channel_without_sanity_checks() {
    assert_eq!(
        parse_ibc_channel_without_sanity_checks("foo").is_err(),
        true
    );
    assert_eq!(
        parse_ibc_channel_without_sanity_checks("foo/bar").is_err(),
        true
    );
    let result = parse_ibc_channel_without_sanity_checks("foo/bar/helloworld").unwrap();
    assert_eq!(result, "bar");

    let result = parse_ibc_info_without_sanity_checks("foo/bar").unwrap_or_default();
    assert_eq!(result.0, "");
    assert_eq!(result.1, "");
    assert_eq!(result.2, "");
}

#[test]
fn test_parse_ibc_info_without_sanity_checks() {
    assert_eq!(parse_ibc_info_without_sanity_checks("foo").is_err(), true);
    assert_eq!(
        parse_ibc_info_without_sanity_checks("foo/bar").is_err(),
        true
    );
    let result = parse_ibc_info_without_sanity_checks("foo/bar/helloworld").unwrap();
    assert_eq!(result.0, "foo");
    assert_eq!(result.1, "bar");
    assert_eq!(result.2, "helloworld");

    let result = parse_ibc_info_without_sanity_checks("foo/bar").unwrap_or_default();
    assert_eq!(result.0, "");
    assert_eq!(result.1, "");
    assert_eq!(result.2, "");
}

#[test]
fn test_deduct_token_fee() {
    let mut deps = mock_dependencies();
    let amount = Uint128::from(1000u64);
    let storage = deps.as_mut().storage;
    let token_fee_denom = "foo0x";
    // should return amount because we have not set relayer fee yet
    assert_eq!(deduct_token_fee(storage, "foo", amount).unwrap().0, amount);
    TOKEN_FEE
        .save(
            storage,
            token_fee_denom,
            &Ratio {
                nominator: 1,
                denominator: 100,
            },
        )
        .unwrap();
    assert_eq!(
        deduct_token_fee(storage, token_fee_denom, amount)
            .unwrap()
            .0,
        Uint128::from(990u64)
    );
}

#[test]
fn test_deduct_relayer_fee() {
    let mut deps = mock_dependencies();
    let deps_mut = deps.as_mut();
    let token_fee_denom = "cosmos";
    let remote_address = "cosmos1zedxv25ah8fksmg2lzrndrpkvsjqgk4zt5ff7n";
    let destination_asset_on_orai = AssetInfo::NativeToken {
        denom: "orai".to_string(),
    };
    let swap_router_contract = RouterController("foo".to_string());
    // token price empty case. Should return zero fee
    let result = deduct_relayer_fee(
        deps_mut.storage,
        deps_mut.api,
        &deps_mut.querier,
        remote_address,
        token_fee_denom,
        destination_asset_on_orai.clone(),
        &swap_router_contract,
    )
    .unwrap();
    assert_eq!(result, Uint128::zero());

    // remote address is wrong (dont follow bech32 form)
    assert_eq!(
        deduct_relayer_fee(
            deps_mut.storage,
            deps_mut.api,
            &deps_mut.querier,
            "foobar",
            token_fee_denom,
            destination_asset_on_orai.clone(),
            &swap_router_contract,
        )
        .unwrap(),
        Uint128::from(0u128)
    );

    // no relayer fee case
    assert_eq!(
        deduct_relayer_fee(
            deps_mut.storage,
            deps_mut.api,
            &deps_mut.querier,
            remote_address,
            token_fee_denom,
            destination_asset_on_orai.clone(),
            &swap_router_contract,
        )
        .unwrap(),
        Uint128::zero()
    );

    // oraib prefix case.
    RELAYER_FEE
        .save(deps_mut.storage, token_fee_denom, &Uint128::from(100u64))
        .unwrap();

    RELAYER_FEE
        .save(deps_mut.storage, "foo", &Uint128::from(1000u64))
        .unwrap();

    assert_eq!(
        deduct_relayer_fee(
            deps_mut.storage,
            deps_mut.api,
            &deps_mut.querier,
            "oraib1603j3e4juddh7cuhfquxspl0p0nsun047wz3rl",
            "foo0x",
            destination_asset_on_orai.clone(),
            &swap_router_contract,
        )
        .unwrap(),
        Uint128::from(1000u64)
    );

    // normal case with remote address
    assert_eq!(
        deduct_relayer_fee(
            deps_mut.storage,
            deps_mut.api,
            &deps_mut.querier,
            remote_address,
            token_fee_denom,
            destination_asset_on_orai,
            &swap_router_contract,
        )
        .unwrap(),
        Uint128::from(100u64)
    );
}

#[test]
fn test_get_swap_token_amount_out_from_orai() {
    let deps = mock_dependencies();
    let simulate_amount = Uint128::from(10u128);
    let result = get_swap_token_amount_out_from_orai(
        &deps.as_ref().querier,
        simulate_amount,
        &RouterController("foo".to_string()),
        AssetInfo::NativeToken {
            denom: "orai".to_string(),
        },
    );
    assert_eq!(result, simulate_amount)
}

#[test]
fn test_split_denom() {
    let split_denom: Vec<&str> = "orai".splitn(3, '/').collect();
    assert_eq!(split_denom.len(), 1);

    let split_denom: Vec<&str> = "a/b/c".splitn(3, '/').collect();
    assert_eq!(split_denom.len(), 3)
}

#[test]
fn setup_and_query() {
    let deps = setup(&["channel-3", "channel-7"], &[]);

    let raw_list = query(deps.as_ref(), mock_env(), QueryMsg::ListChannels {}).unwrap();
    let list_res: ListChannelsResponse = from_json(&raw_list).unwrap();
    assert_eq!(2, list_res.channels.len());
    assert_eq!(mock_channel_info("channel-3"), list_res.channels[0]);
    assert_eq!(mock_channel_info("channel-7"), list_res.channels[1]);

    let raw_channel = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::Channel {
            id: "channel-3".to_string(),
        },
    )
    .unwrap();
    let chan_res: ChannelResponse = from_json(&raw_channel).unwrap();
    assert_eq!(chan_res.info, mock_channel_info("channel-3"));
    assert_eq!(0, chan_res.total_sent.len());
    assert_eq!(0, chan_res.balances.len());

    let err = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::Channel {
            id: "channel-10".to_string(),
        },
    )
    .unwrap_err();
    assert!(err
        .to_string()
        .contains("type: cw20_ics20_msg::state::ChannelInfo;"));
    assert!(err.to_string().contains("not found"));
}

#[test]
fn test_query_pair_mapping_by_asset_info() {
    let mut deps = setup(&["channel-3", "channel-7"], &[]);
    let asset_info = AssetInfo::Token {
        contract_addr: Addr::unchecked("cw20:foobar".to_string()),
    };
    let mut update = UpdatePairMsg {
        local_channel_id: "mars-channel".to_string(),
        denom: "earth".to_string(),
        local_asset_info: asset_info.clone(),
        remote_decimals: 18,
        local_asset_info_decimals: 18,
        is_mint_burn: None,
    };

    // works with proper funds
    let mut msg = ExecuteMsg::UpdateMappingPair(update.clone());

    let info = mock_info("gov", &coins(1234567, "ucosm"));
    execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();

    // add another pair with the same asset info to filter
    update.denom = "jupiter".to_string();
    msg = ExecuteMsg::UpdateMappingPair(update.clone());
    let info = mock_info("gov", &coins(1234567, "ucosm"));
    execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();

    // add another pair with a different asset info
    update.denom = "moon".to_string();
    update.local_asset_info = AssetInfo::NativeToken {
        denom: "orai".to_string(),
    };
    msg = ExecuteMsg::UpdateMappingPair(update.clone());
    let info = mock_info("gov", &coins(1234567, "ucosm"));
    execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();

    // query based on asset info

    let mappings = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::PairMappingsFromAssetInfo {
            asset_info: asset_info,
        },
    )
    .unwrap();
    let response: Vec<PairQuery> = from_json(&mappings).unwrap();
    assert_eq!(response.len(), 2);

    // query native token asset info, should receive moon denom in key
    let mappings = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::PairMappingsFromAssetInfo {
            asset_info: AssetInfo::NativeToken {
                denom: "orai".to_string(),
            },
        },
    )
    .unwrap();
    let response: Vec<PairQuery> = from_json(&mappings).unwrap();
    assert_eq!(response.len(), 1);
    assert_eq!(response.first().unwrap().key.contains("moon"), true);

    // query asset info that is not in the mapping, should return empty
    // query native token asset info, should receive moon denom
    let mappings = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::PairMappingsFromAssetInfo {
            asset_info: AssetInfo::NativeToken {
                denom: "foobar".to_string(),
            },
        },
    )
    .unwrap();
    let response: Vec<PairQuery> = from_json(&mappings).unwrap();
    assert_eq!(response.len(), 0);
}

#[test]
fn test_update_cw20_mapping() {
    let mut deps = setup(&["channel-3", "channel-7"], &[]);
    let asset_info = AssetInfo::Token {
        contract_addr: Addr::unchecked("cw20:foobar".to_string()),
    };
    let asset_info_second = AssetInfo::Token {
        contract_addr: Addr::unchecked("cw20:foobar-second".to_string()),
    };

    let mut update = UpdatePairMsg {
        local_channel_id: "mars-channel".to_string(),
        denom: "earth".to_string(),
        local_asset_info: asset_info.clone(),
        remote_decimals: 18,
        local_asset_info_decimals: 18,
        is_mint_burn: None,
    };

    // works with proper funds
    let mut msg = ExecuteMsg::UpdateMappingPair(update.clone());

    // unauthorized case
    let info = mock_info("foobar", &coins(1234567, "ucosm"));
    let res_err = execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap_err();
    assert_eq!(res_err, ContractError::Admin(AdminError::NotAdmin {}));

    let info = mock_info("gov", &coins(1234567, "ucosm"));
    execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();

    // query to verify if the mapping has been updated
    let mappings = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::PairMappings {
            start_after: None,
            limit: None,
            order: None,
        },
    )
    .unwrap();
    let response: ListMappingResponse = from_json(&mappings).unwrap();
    println!("response: {:?}", response);
    assert_eq!(
        response.pairs.first().unwrap().key,
        format!("{}/mars-channel/earth", CONTRACT_PORT)
    );

    // not found case
    let mappings = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::PairMappings {
            start_after: None,
            limit: None,
            order: None,
        },
    )
    .unwrap();
    let response: ListMappingResponse = from_json(&mappings).unwrap();
    assert_ne!(response.pairs.first().unwrap().key, "foobar".to_string());

    // update existing key case must pass
    update.local_asset_info = asset_info_second.clone();
    msg = ExecuteMsg::UpdateMappingPair(update.clone());

    let info = mock_info("gov", &coins(1234567, "ucosm"));
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    // after update, cw20 denom now needs to be updated
    let mappings = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::PairMappings {
            start_after: None,
            limit: None,
            order: None,
        },
    )
    .unwrap();
    let response: ListMappingResponse = from_json(&mappings).unwrap();
    println!("response: {:?}", response);
    assert_eq!(
        response.pairs.first().unwrap().key,
        format!("{}/mars-channel/earth", CONTRACT_PORT)
    );
    assert_eq!(
        response.pairs.first().unwrap().pair_mapping.asset_info,
        asset_info_second
    )
}

#[test]
fn test_delete_cw20_mapping() {
    let mut deps = setup(&["channel-3", "channel-7"], &[]);
    let cw20_denom = AssetInfo::Token {
        contract_addr: Addr::unchecked("cw20:foobar".to_string()),
    };

    let update = UpdatePairMsg {
        local_channel_id: "mars-channel".to_string(),
        denom: "earth".to_string(),
        local_asset_info: cw20_denom.clone(),
        remote_decimals: 18,
        local_asset_info_decimals: 18,
        is_mint_burn: None,
    };

    // works with proper funds
    let msg = ExecuteMsg::UpdateMappingPair(update.clone());

    let info = mock_info("gov", &coins(1234567, "ucosm"));
    execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();

    // query to verify if the mapping has been updated
    let mappings = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::PairMappings {
            start_after: None,
            limit: None,
            order: None,
        },
    )
    .unwrap();
    let response: ListMappingResponse = from_json(&mappings).unwrap();
    println!("response: {:?}", response);
    assert_eq!(
        response.pairs.first().unwrap().key,
        format!("{}/mars-channel/earth", CONTRACT_PORT)
    );

    // now try deleting
    let delete = DeletePairMsg {
        local_channel_id: "mars-channel".to_string(),
        denom: "earth".to_string(),
    };

    let mut msg = ExecuteMsg::DeleteMappingPair(delete.clone());

    // unauthorized delete case
    let info = mock_info("foobar", &coins(1234567, "ucosm"));
    let delete_err = execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap_err();
    assert_eq!(delete_err, ContractError::Admin(AdminError::NotAdmin {}));

    let info = mock_info("gov", &coins(1234567, "ucosm"));

    // happy case
    msg = ExecuteMsg::DeleteMappingPair(delete.clone());
    execute(deps.as_mut(), mock_env(), info.clone(), msg.clone()).unwrap();

    // after update, the list cw20 mapping should be empty
    let mappings = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::PairMappings {
            start_after: None,
            limit: None,
            order: None,
        },
    )
    .unwrap();
    let response: ListMappingResponse = from_json(&mappings).unwrap();
    println!("response: {:?}", response);
    assert_eq!(response.pairs.len(), 0)
}

// #[test]
// fn proper_checks_on_execute_native() {
//     let send_channel = "channel-5";
//     let mut deps = setup(&[send_channel, "channel-10"], &[]);

//     let mut transfer = TransferMsg {
//         channel: send_channel.to_string(),
//         remote_address: "foreign-address".to_string(),
//         timeout: None,
//         memo: Some("memo".to_string()),
//     };

//     // works with proper funds
//     let msg = ExecuteMsg::Transfer(transfer.clone());
//     let info = mock_info("foobar", &coins(1234567, "ucosm"));
//     let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
//     assert_eq!(res.messages[0].gas_limit, None);
//     assert_eq!(1, res.messages.len());
//     if let CosmosMsg::Ibc(IbcMsg::SendPacket {
//         channel_id,
//         data,
//         timeout,
//     }) = &res.messages[0].msg
//     {
//         let expected_timeout = mock_env().block.time.plus_seconds(DEFAULT_TIMEOUT);
//         assert_eq!(timeout, &expected_timeout.into());
//         assert_eq!(channel_id.as_str(), send_channel);
//         let msg: Ics20Packet = from_json(data).unwrap();
//         assert_eq!(msg.amount, Uint128::new(1234567));
//         assert_eq!(msg.denom.as_str(), "ucosm");
//         assert_eq!(msg.sender.as_str(), "foobar");
//         assert_eq!(msg.receiver.as_str(), "foreign-address");
//     } else {
//         panic!("Unexpected return message: {:?}", res.messages[0]);
//     }

//     // reject with no funds
//     let msg = ExecuteMsg::Transfer(transfer.clone());
//     let info = mock_info("foobar", &[]);
//     let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
//     assert_eq!(err, ContractError::Payment(PaymentError::NoFunds {}));

//     // reject with multiple tokens funds
//     let msg = ExecuteMsg::Transfer(transfer.clone());
//     let info = mock_info("foobar", &[coin(1234567, "ucosm"), coin(54321, "uatom")]);
//     let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
//     assert_eq!(err, ContractError::Payment(PaymentError::MultipleDenoms {}));

//     // reject with bad channel id
//     transfer.channel = "channel-45".to_string();
//     let msg = ExecuteMsg::Transfer(transfer);
//     let info = mock_info("foobar", &coins(1234567, "ucosm"));
//     let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
//     assert_eq!(
//         err,
//         ContractError::NoSuchChannel {
//             id: "channel-45".to_string()
//         }
//     );
// }

// #[test]
// fn proper_checks_on_execute_cw20() {
//     let send_channel = "channel-15";
//     let cw20_addr = "my-token";
//     let mut deps = setup(&["channel-3", send_channel], &[(cw20_addr, 123456)]);

//     let transfer = TransferMsg {
//         channel: send_channel.to_string(),
//         remote_address: "foreign-address".to_string(),
//         timeout: Some(7777),
//         memo: Some("memo".to_string()),
//     };
//     let msg = ExecuteMsg::Receive(Cw20ReceiveMsg {
//         sender: "my-account".into(),
//         amount: Uint128::new(888777666),
//         msg: to_json_binary(&transfer).unwrap(),
//     });

//     // works with proper funds
//     let info = mock_info(cw20_addr, &[]);
//     let res = execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();
//     assert_eq!(1, res.messages.len());
//     assert_eq!(res.messages[0].gas_limit, None);
//     if let CosmosMsg::Ibc(IbcMsg::SendPacket {
//         channel_id,
//         data,
//         timeout,
//     }) = &res.messages[0].msg
//     {
//         let expected_timeout = mock_env().block.time.plus_seconds(7777);
//         assert_eq!(timeout, &expected_timeout.into());
//         assert_eq!(channel_id.as_str(), send_channel);
//         let msg: Ics20Packet = from_json(data).unwrap();
//         assert_eq!(msg.amount, Uint128::new(888777666));
//         assert_eq!(msg.denom, format!("cw20:{}", cw20_addr));
//         assert_eq!(msg.sender.as_str(), "my-account");
//         assert_eq!(msg.receiver.as_str(), "foreign-address");
//     } else {
//         panic!("Unexpected return message: {:?}", res.messages[0]);
//     }

//     // reject with tokens funds
//     let info = mock_info("foobar", &coins(1234567, "ucosm"));
//     let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
//     assert_eq!(err, ContractError::Payment(PaymentError::NonPayable {}));
// }

// #[test]
// fn execute_cw20_fails_if_not_whitelisted_unless_default_gas_limit() {
//     let send_channel = "channel-15";
//     let mut deps = setup(&[send_channel], &[]);

//     let cw20_addr = "my-token";
//     let transfer = TransferMsg {
//         channel: send_channel.to_string(),
//         remote_address: "foreign-address".to_string(),
//         timeout: Some(7777),
//         memo: Some("memo".to_string()),
//     };
//     let msg = ExecuteMsg::Receive(Cw20ReceiveMsg {
//         sender: "my-account".into(),
//         amount: Uint128::new(888777666),
//         msg: to_json_binary(&transfer).unwrap(),
//     });

//     // rejected as not on allow list
//     let info = mock_info(cw20_addr, &[]);
//     let err = execute(deps.as_mut(), mock_env(), info.clone(), msg.clone()).unwrap_err();
//     assert_eq!(err, ContractError::NotOnAllowList);

//     // add a default gas limit
//     migrate(
//         deps.as_mut(),
//         mock_env(),
//         MigrateMsg {
//             default_gas_limit: Some(123456),
//             fee_receiver: "receiver".to_string(),
//             default_timeout: 100u64,
//             fee_denom: "orai".to_string(),
//             swap_router_contract: "foobar".to_string(),
//         },
//     )
//     .unwrap();

//     // try again
//     execute(deps.as_mut(), mock_env(), info, msg).unwrap();
// }
// test execute transfer back to native remote chain

fn mock_receive_packet(
    remote_channel: &str,
    local_channel: &str,
    amount: Uint128,
    denom: String,
    receiver: String,
) -> IbcPacket {
    let data = Ics20Packet {
        // this is returning a foreign (our) token, thus denom is <port>/<channel>/<denom>
        denom,
        amount,
        sender: "remote-sender".to_string(),
        receiver,
        memo: Some("memo".to_string()),
    };
    IbcPacket::new(
        to_json_binary(&data).unwrap(),
        IbcEndpoint {
            port_id: REMOTE_PORT.to_string(),
            channel_id: remote_channel.to_string(),
        },
        IbcEndpoint {
            port_id: CONTRACT_PORT.to_string(),
            channel_id: local_channel.to_string(),
        },
        3,
        Timestamp::from_seconds(1665321069).into(),
    )
}

#[test]
fn proper_checks_on_execute_cw20_transfer_back_to_remote() {
    // arrange
    let relayer = Addr::unchecked("relayer");
    let remote_channel = "channel-5";
    let remote_address = "cosmos1603j3e4juddh7cuhfquxspl0p0nsun046us7n0";
    let custom_addr = "custom-addr";
    let original_sender = "original_sender";
    let denom = "uatom0x";
    let amount = Uint128::from(1234567u128);
    let asset_info = AssetInfo::NativeToken {
        denom: denom.into(),
    };
    let cw20_raw_denom = original_sender;
    let local_channel = "channel-1234";
    let ibc_denom = get_key_ics20_ibc_denom("wasm.cosmos2contract", local_channel, denom);
    let ratio = Ratio {
        nominator: 1,
        denominator: 10,
    };
    let fee_amount = amount * Decimal::from_ratio(ratio.nominator, ratio.denominator);
    let mut deps = setup(&[remote_channel, local_channel], &[]);
    TOKEN_FEE
        .save(deps.as_mut().storage, denom, &ratio)
        .unwrap();

    let pair = UpdatePairMsg {
        local_channel_id: local_channel.to_string(),
        denom: denom.to_string(),
        local_asset_info: asset_info.clone(),
        remote_decimals: 18u8,
        local_asset_info_decimals: 18u8,
        is_mint_burn: None,
    };

    let _ = execute(
        deps.as_mut(),
        mock_env(),
        mock_info("gov", &[]),
        ExecuteMsg::UpdateMappingPair(pair),
    )
    .unwrap();

    // execute
    let mut transfer = TransferBackMsg {
        local_channel_id: local_channel.to_string(),
        remote_address: remote_address.to_string(),
        remote_denom: denom.to_string(),
        timeout: Some(DEFAULT_TIMEOUT),
        memo: None,
    };

    let msg = ExecuteMsg::TransferToRemote(transfer.clone());

    // insufficient funds case because we need to receive from remote chain first
    let info = mock_info(
        cw20_raw_denom,
        &[Coin {
            amount,
            denom: denom.to_string(),
        }],
    );
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg.clone());
    assert_eq!(
        res.unwrap_err(),
        ContractError::NoSuchChannelState {
            id: local_channel.to_string(),
            denom: ibc_denom.clone()
        }
    );

    // prepare some mock packets
    let recv_packet = mock_receive_packet(
        remote_channel,
        local_channel,
        amount,
        denom.to_string(),
        custom_addr.to_string(),
    );

    // receive some tokens. Assume that the function works perfectly because the test case is elsewhere
    let ibc_msg = IbcPacketReceiveMsg::new(recv_packet.clone(), relayer);
    ibc_packet_receive(deps.as_mut(), mock_env(), ibc_msg).unwrap();
    // need to trigger increase channel balance because it is executed through submsg
    execute(
        deps.as_mut(),
        mock_env(),
        mock_info(mock_env().contract.address.as_str(), &[]),
        ExecuteMsg::IncreaseChannelBalanceIbcReceive {
            dest_channel_id: local_channel.to_string(),
            ibc_denom: ibc_denom.clone(),
            amount,
            local_receiver: custom_addr.to_string(),
        },
    )
    .unwrap();

    // error cases
    // revert transfer state to correct state
    transfer.local_channel_id = local_channel.to_string();
    let msg: ExecuteMsg = ExecuteMsg::TransferToRemote(transfer.clone());

    // now we execute transfer back to remote chain
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    assert_eq!(res.messages[0].gas_limit, None);
    println!("res messages: {:?}", res.messages);
    assert_eq!(2, res.messages.len()); // 2 because it also has deduct fee msg
    match res.messages[1].msg.clone() {
        CosmosMsg::Ibc(IbcMsg::SendPacket {
            channel_id,
            data,
            timeout,
        }) => {
            let expected_timeout = DEFAULT_TIMEOUT;
            assert_eq!(timeout.timestamp().unwrap().nanos(), expected_timeout);
            assert_eq!(channel_id.as_str(), local_channel);
            let msg: Ics20Packet = from_json(&data).unwrap();
            assert_eq!(msg.amount, Uint128::new(1234567).sub(fee_amount));
            assert_eq!(
                msg.denom.as_str(),
                get_key_ics20_ibc_denom(CONTRACT_PORT, local_channel, denom)
            );
            assert_eq!(msg.sender.as_str(), original_sender);
            assert_eq!(msg.receiver.as_str(), remote_address);
            // assert_eq!(msg.memo, None);
        }
        _ => panic!("Unexpected return message: {:?}", res.messages[0]),
    }
    match res.messages[0].msg.clone() {
        CosmosMsg::Bank(BankMsg::Send {
            to_address,
            amount: message_amount,
        }) => {
            assert_eq!(to_address, "gov".to_string());
            assert_eq!(message_amount, coins(fee_amount.u128(), denom));
        }
        _ => panic!("Unexpected return message: {:?}", res.messages[0]),
    }

    // check new channel state after reducing balance
    let chan = query_channel(deps.as_ref(), local_channel.into()).unwrap();
    assert_eq!(
        chan.balances,
        vec![Amount::native(
            fee_amount,
            get_key_ics20_ibc_denom(CONTRACT_PORT, local_channel, denom)
        )]
    );
    assert_eq!(
        chan.total_sent,
        vec![Amount::native(
            amount,
            get_key_ics20_ibc_denom(CONTRACT_PORT, local_channel, denom)
        )]
    );

    // mapping pair error with wrong voucher denom
    let pair = UpdatePairMsg {
        local_channel_id: "not_registered_channel".to_string(),
        denom: denom.to_string(),
        local_asset_info: AssetInfo::Token {
            contract_addr: Addr::unchecked("random_cw20_denom".to_string()),
        },
        remote_decimals: 18u8,
        local_asset_info_decimals: 18u8,
        is_mint_burn: None,
    };

    execute(
        deps.as_mut(),
        mock_env(),
        mock_info("gov", &[]),
        ExecuteMsg::UpdateMappingPair(pair),
    )
    .unwrap();

    transfer.local_channel_id = "not_registered_channel".to_string();
    let invalid_msg = ExecuteMsg::TransferToRemote(transfer);
    let err = execute(deps.as_mut(), mock_env(), info.clone(), invalid_msg).unwrap_err();
    assert_eq!(err, ContractError::MappingPairNotFound {});
}

#[test]
fn test_update_config() {
    // arrange
    let mut deps = setup(&[], &[]);
    let new_config = ExecuteMsg::UpdateConfig {
        admin: Some("helloworld".to_string()),
        default_timeout: Some(1),
        default_gas_limit: None,
        swap_router_contract: Some("new_router".to_string()),
        token_fee: Some(vec![
            TokenFee {
                token_denom: "orai".to_string(),
                ratio: Ratio {
                    nominator: 1,
                    denominator: 10,
                },
            },
            TokenFee {
                token_denom: "atom".to_string(),
                ratio: Ratio {
                    nominator: 1,
                    denominator: 5,
                },
            },
        ]),
        relayer_fee: Some(vec![RelayerFee {
            prefix: "foo".to_string(),
            fee: Uint128::from(1000000u64),
        }]),
        fee_receiver: Some("token_fee_receiver".to_string()),
        relayer_fee_receiver: Some("relayer_fee_receiver".to_string()),
        converter_contract: Some("new_converter".to_string()),
        osor_entrypoint_contract: Some("new_osor_contract".to_string()),
        token_factory_addr: Some("new_token_factory_addr".to_string()),
    };
    // unauthorized case
    let unauthorized_info = mock_info(&String::from("somebody"), &[]);
    let is_err = execute(
        deps.as_mut(),
        mock_env(),
        unauthorized_info,
        new_config.clone(),
    )
    .is_err();
    assert_eq!(is_err, true);
    // valid case
    let info = mock_info(&String::from("gov"), &[]);
    execute(deps.as_mut(), mock_env(), info, new_config).unwrap();
    let config: ConfigResponse =
        from_json(&query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap()).unwrap();
    assert_eq!(config.default_gas_limit, None);
    assert_eq!(config.default_timeout, 1);
    assert_eq!(config.swap_router_contract, "new_router".to_string());
    assert_eq!(
        config.relayer_fee_receiver,
        Addr::unchecked("relayer_fee_receiver")
    );
    assert_eq!(
        config.token_fee_receiver,
        Addr::unchecked("token_fee_receiver")
    );
    assert_eq!(
        config.osor_entrypoint_contract,
        Addr::unchecked("new_osor_contract")
    );
    assert_eq!(config.token_fees.len(), 2usize);
    assert_eq!(config.token_fees[0].ratio.denominator, 5);
    assert_eq!(config.token_fees[0].token_denom, "atom".to_string());
    assert_eq!(config.token_fees[1].ratio.denominator, 10);
    assert_eq!(config.token_fees[1].token_denom, "orai".to_string());
    assert_eq!(config.relayer_fees.len(), 1);
    assert_eq!(config.relayer_fees[0].prefix, "foo".to_string());
    assert_eq!(config.relayer_fees[0].amount, Uint128::from(1000000u64));
}

#[test]
fn test_asset_info() {
    let asset_info = AssetInfo::NativeToken {
        denom: "orai".to_string(),
    };
    assert_eq!(asset_info.to_string(), "orai".to_string());
    let asset_info = AssetInfo::Token {
        contract_addr: Addr::unchecked("oraiaxbc".to_string()),
    };
    assert_eq!(asset_info.to_string(), "oraiaxbc".to_string())
}

#[test]
fn test_handle_packet_refund() {
    let local_channel_id = "channel-0";
    let mut deps = setup(&[local_channel_id], &[]);
    let native_denom = "cosmos";
    let amount = Uint128::from(100u128);
    let sender = "sender";
    let local_asset_info = AssetInfo::NativeToken {
        denom: "orai".to_string(),
    };
    let mapping_denom = format!("wasm.cosmos2contract/{}/{}", local_channel_id, native_denom);

    let result = handle_packet_refund(deps.as_mut().storage, sender, native_denom, amount, false)
        .unwrap_err();
    assert!(result
        .to_string()
        .contains("cw20_ics20_msg::state::MappingMetadata"));
    assert!(result.to_string().contains("not found"));

    // update mapping pair so that we can get refunded
    // cosmos based case with mapping found. Should be successful & cosmos msg is ibc send packet
    // add a pair mapping so we can test the happy case evm based happy case
    let mut update = UpdatePairMsg {
        local_channel_id: local_channel_id.to_string(),
        denom: native_denom.to_string(),
        local_asset_info: local_asset_info.clone(),
        remote_decimals: 6,
        local_asset_info_decimals: 6,
        is_mint_burn: None,
    };

    let msg = ExecuteMsg::UpdateMappingPair(update.clone());

    let info = mock_info("gov", &coins(1234567, "ucosm"));
    execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();

    // now we handle packet failure. should get sub msg
    let result =
        handle_packet_refund(deps.as_mut().storage, sender, &mapping_denom, amount, false).unwrap();
    assert_eq!(
        result,
        SubMsg::reply_on_error(
            CosmosMsg::Bank(BankMsg::Send {
                to_address: sender.to_string(),
                amount: coins(amount.u128(), "orai")
            }),
            REFUND_FAILURE_ID
        )
    );

    // case 2: refunds with mint msg
    let local_asset_info = AssetInfo::Token {
        contract_addr: Addr::unchecked("token0"),
    };
    update.local_asset_info = local_asset_info;
    update.is_mint_burn = Some(true);
    let msg = ExecuteMsg::UpdateMappingPair(update.clone());
    let info = mock_info("gov", &coins(1234567, "ucosm"));
    execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();
    let result =
        handle_packet_refund(deps.as_mut().storage, sender, &mapping_denom, amount, true).unwrap();
    assert_eq!(
        result,
        SubMsg::reply_on_error(
            CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: "token0".to_string(),
                msg: to_json_binary(&Cw20ExecuteMsg::Mint {
                    recipient: sender.to_string(),
                    amount
                })
                .unwrap(),
                funds: vec![]
            }),
            REFUND_FAILURE_ID
        )
    );
}

#[test]
fn test_increase_channel_balance_ibc_receive() {
    let local_channel_id = "channel-0";
    let amount = Uint128::from(10u128);
    let ibc_denom = "foobar";
    let local_receiver = "receiver";
    let mut deps = setup(&[local_channel_id], &[]);

    let local_asset_info = AssetInfo::NativeToken {
        denom: "orai".to_string(),
    };
    let ibc_denom_keys = format!(
        "wasm.{}/{}/{}",
        mock_env().contract.address.to_string(),
        local_channel_id,
        ibc_denom
    );

    // register mapping
    let update = UpdatePairMsg {
        local_channel_id: local_channel_id.to_string(),
        denom: ibc_denom.to_string(),
        local_asset_info: local_asset_info.clone(),
        remote_decimals: 6,
        local_asset_info_decimals: 6,
        is_mint_burn: None,
    };
    execute(
        deps.as_mut(),
        mock_env(),
        mock_info("gov", &vec![]),
        ExecuteMsg::UpdateMappingPair(update),
    )
    .unwrap();

    assert_eq!(
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info("attacker", &vec![]),
            ExecuteMsg::IncreaseChannelBalanceIbcReceive {
                dest_channel_id: local_channel_id.to_string(),
                ibc_denom: ibc_denom_keys.to_string(),
                amount: amount.clone(),
                local_receiver: local_receiver.to_string(),
            },
        )
        .unwrap_err(),
        ContractError::Std(StdError::generic_err("Caller is not the contract itself!"))
    );

    execute(
        deps.as_mut(),
        mock_env(),
        mock_info(mock_env().contract.address.as_str(), &vec![]),
        ExecuteMsg::IncreaseChannelBalanceIbcReceive {
            dest_channel_id: local_channel_id.to_string(),
            ibc_denom: ibc_denom_keys.to_string(),
            amount: amount.clone(),
            local_receiver: local_receiver.to_string(),
        },
    )
    .unwrap();
    let channel_state = CHANNEL_REVERSE_STATE
        .load(deps.as_ref().storage, (local_channel_id, &ibc_denom_keys))
        .unwrap();
    assert_eq!(channel_state.outstanding, amount);
    assert_eq!(channel_state.total_sent, amount);
    let reply_args = REPLY_ARGS.load(deps.as_ref().storage).unwrap();
    assert_eq!(reply_args.amount, amount);
    assert_eq!(reply_args.channel, local_channel_id);
    assert_eq!(reply_args.denom, ibc_denom_keys.to_string());
    assert_eq!(reply_args.local_receiver, local_receiver.to_string());
}

#[test]
fn test_reduce_channel_balance_ibc_receive() {
    let local_channel_id = "channel-0";
    let amount = Uint128::from(10u128);
    let ibc_denom = "foobar";
    let local_receiver = "receiver";
    let mut deps = setup(&[local_channel_id], &[]);
    let local_asset_info = AssetInfo::NativeToken {
        denom: "orai".to_string(),
    };

    let ibc_denom_keys = format!(
        "wasm.{}/{}/{}",
        mock_env().contract.address.to_string(),
        local_channel_id,
        ibc_denom
    );

    // register mapping
    let update = UpdatePairMsg {
        local_channel_id: local_channel_id.to_string(),
        denom: ibc_denom.to_string(),
        local_asset_info: local_asset_info.clone(),
        remote_decimals: 6,
        local_asset_info_decimals: 6,
        is_mint_burn: None,
    };
    execute(
        deps.as_mut(),
        mock_env(),
        mock_info("gov", &vec![]),
        ExecuteMsg::UpdateMappingPair(update),
    )
    .unwrap();

    execute(
        deps.as_mut(),
        mock_env(),
        mock_info(mock_env().contract.address.as_str(), &vec![]),
        ExecuteMsg::IncreaseChannelBalanceIbcReceive {
            dest_channel_id: local_channel_id.to_string(),
            ibc_denom: ibc_denom_keys.to_string(),
            amount: amount.clone(),
            local_receiver: local_receiver.to_string(),
        },
    )
    .unwrap();

    assert_eq!(
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info("attacker", &vec![]),
            ExecuteMsg::ReduceChannelBalanceIbcReceive {
                src_channel_id: local_channel_id.to_string(),
                ibc_denom: ibc_denom_keys.to_string(),
                amount: amount.clone(),
                local_receiver: local_receiver.to_string(),
            },
        )
        .unwrap_err(),
        ContractError::Std(StdError::generic_err("Caller is not the contract itself!"))
    );

    execute(
        deps.as_mut(),
        mock_env(),
        mock_info(mock_env().contract.address.as_str(), &vec![]),
        ExecuteMsg::ReduceChannelBalanceIbcReceive {
            src_channel_id: local_channel_id.to_string(),
            ibc_denom: ibc_denom_keys.to_string(),
            amount: amount.clone(),
            local_receiver: local_receiver.to_string(),
        },
    )
    .unwrap();
    let channel_state = CHANNEL_REVERSE_STATE
        .load(deps.as_ref().storage, (local_channel_id, &ibc_denom_keys))
        .unwrap();
    assert_eq!(channel_state.outstanding, Uint128::zero());
    assert_eq!(channel_state.total_sent, Uint128::from(10u128));
    let reply_args = REPLY_ARGS.load(deps.as_ref().storage).unwrap();
    assert_eq!(reply_args.amount, amount);
    assert_eq!(reply_args.channel, local_channel_id);
    assert_eq!(reply_args.denom, ibc_denom_keys);
    assert_eq!(reply_args.local_receiver, local_receiver.to_string());
}

#[test]
fn test_query_channel_balance_with_key() {
    // fixture
    let channel = "foo-channel";
    let ibc_denom = "port/channel/denom";
    let amount = Uint128::from(10u128);
    let reduce_amount = Uint128::from(1u128);
    let mut deps = setup(&[channel], &[]);
    increase_channel_balance(deps.as_mut().storage, channel, ibc_denom, amount).unwrap();
    reduce_channel_balance(
        deps.as_mut().storage,
        channel,
        ibc_denom,
        Uint128::from(1u128),
    )
    .unwrap();

    let result =
        query_channel_with_key(deps.as_ref(), channel.to_string(), ibc_denom.to_string()).unwrap();
    assert_eq!(
        result.balance,
        Amount::from_parts(
            ibc_denom.to_string(),
            amount.checked_sub(reduce_amount).unwrap()
        )
    );
    assert_eq!(
        result.total_sent,
        Amount::from_parts(ibc_denom.to_string(), amount)
    );
}

#[test]
fn test_handle_override_channel_balance() {
    // fixture
    let channel = "foo-channel";
    let ibc_denom = "port/channel/denom";
    let amount = Uint128::from(10u128);
    let override_amount = Uint128::from(100u128);
    let total_sent_override = Uint128::from(1000u128);
    let mut deps = setup(&[channel], &[]);
    increase_channel_balance(deps.as_mut().storage, channel, ibc_denom, amount).unwrap();

    // unauthorized case
    let unauthorized = handle_override_channel_balance(
        deps.as_mut(),
        mock_info("attacker", &vec![]),
        channel.to_string(),
        ibc_denom.to_string(),
        amount,
        None,
    )
    .unwrap_err();
    assert_eq!(unauthorized, ContractError::Admin(AdminError::NotAdmin {}));

    // execution, valid case
    handle_override_channel_balance(
        deps.as_mut(),
        mock_info("gov", &vec![]),
        channel.to_string(),
        ibc_denom.to_string(),
        override_amount,
        Some(total_sent_override),
    )
    .unwrap();

    // we query to validate the result after overriding

    let result =
        query_channel_with_key(deps.as_ref(), channel.to_string(), ibc_denom.to_string()).unwrap();
    assert_eq!(
        result.balance,
        Amount::from_parts(ibc_denom.to_string(), override_amount)
    );
    assert_eq!(
        result.total_sent,
        Amount::from_parts(ibc_denom.to_string(), total_sent_override)
    );
}

#[test]
fn test_get_destination_info_on_orai() {
    let mut deps = setup(&["channel-3", "channel-7"], &[]);
    let asset_info = AssetInfo::Token {
        contract_addr: Addr::unchecked("cw20:foobar".to_string()),
    };
    let update = UpdatePairMsg {
        local_channel_id: "mars-channel".to_string(),
        denom: "earth".to_string(),
        local_asset_info: asset_info.clone(),
        remote_decimals: 18,
        local_asset_info_decimals: 18,
        is_mint_burn: None,
    };

    // works with proper funds
    let msg = ExecuteMsg::UpdateMappingPair(update.clone());

    let info = mock_info("gov", &coins(1234567, "ucosm"));
    execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();

    //case 1: destination asset on Oraichain
    let destination_info = get_destination_info_on_orai(
        deps.as_ref().storage,
        deps.as_ref().api,
        &mock_env(),
        "",
        "orai1lus0f0rhx8s03gdllx2n6vhkmf0536dv57wfge",
    );
    assert_eq!(
        destination_info.0,
        AssetInfo::Token {
            contract_addr: Addr::unchecked("orai1lus0f0rhx8s03gdllx2n6vhkmf0536dv57wfge")
        }
    );
    assert_eq!(destination_info.1, None);

    // case 2: Destination asset was registered in mapping pair
    let destination_info = get_destination_info_on_orai(
        deps.as_ref().storage,
        deps.as_ref().api,
        &mock_env(),
        "mars-channel",
        "earth",
    );
    assert_eq!(
        destination_info.0,
        AssetInfo::Token {
            contract_addr: Addr::unchecked("cw20:foobar".to_string())
        }
    );
    assert_eq!(
        destination_info.1,
        Some(PairQuery {
            key: format!(
                "wasm.{}/{}/{}",
                mock_env().contract.address,
                "mars-channel",
                "earth"
            ),
            pair_mapping: MappingMetadata {
                asset_info: AssetInfo::Token {
                    contract_addr: Addr::unchecked("cw20:foobar".to_string())
                },
                remote_decimals: 18,
                asset_info_decimals: 18,
                is_mint_burn: false
            }
        })
    );

    // case 3: Destination asset wasn't registered in mapping pair
    let destination_info = get_destination_info_on_orai(
        deps.as_ref().storage,
        deps.as_ref().api,
        &mock_env(),
        "channel-15",
        "uatom",
    );
    assert_eq!(
        destination_info.0,
        AssetInfo::NativeToken {
            denom: "ibc/A2E2EEC9057A4A1C2C0A6A4C78B0239118DF5F278830F50B4A6BDD7A66506B78"
                .to_string()
        }
    );
    assert_eq!(destination_info.1, None);
}

#[test]
fn test_build_mint_mapping_msg() {
    let mut deps = setup(&["channel-3", "channel-7"], &[]);
    let ibc_denom = "cosmos";
    let local_channel_id = "channel-3";
    let asset_info = AssetInfo::Token {
        contract_addr: Addr::unchecked("cw20:foobar".to_string()),
    };
    let token_factory = "token_factory".to_string();

    let amount_local = Uint128::from(10000u128);
    let receiver = "receiver";

    // case 1: on mappinglist, but mapping mechanism is not mint burn
    let mut update = UpdatePairMsg {
        local_channel_id: local_channel_id.to_string(),
        denom: ibc_denom.to_string(),
        local_asset_info: asset_info.clone(),
        remote_decimals: 18,
        local_asset_info_decimals: 18,
        is_mint_burn: None,
    };
    let msg = ExecuteMsg::UpdateMappingPair(update.clone());
    let info = mock_info("gov", &coins(1234567, "ucosm"));
    execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();
    let res = build_mint_mapping_msg(
        token_factory.clone(),
        false,
        asset_info.clone(),
        amount_local,
        receiver.to_string(),
    );
    assert_eq!(res, Ok(None));

    // case 2: on mappinglist, is mint burn but asset info is native
    let res = build_mint_mapping_msg(
        token_factory.clone(),
        true,
        AssetInfo::NativeToken {
            denom: "orai".to_string(),
        }
        .clone(),
        amount_local,
        receiver.to_string(),
    )
    .unwrap();
    assert_eq!(
        res,
        Some(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: token_factory.clone(),
            msg: to_json_binary(&tokenfactory::msg::ExecuteMsg::MintTokens {
                denom: "orai".to_string(),
                amount: Uint128::from(10000u128),
                mint_to_address: receiver.to_string(),
            })
            .unwrap(),
            funds: vec![],
        }))
    );

    // case 3: got mint msg
    update.is_mint_burn = Some(true);
    let msg = ExecuteMsg::UpdateMappingPair(update.clone());
    let info = mock_info("gov", &coins(1234567, "ucosm"));
    execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();
    let res = build_mint_mapping_msg(
        token_factory.clone(),
        true,
        asset_info,
        amount_local,
        receiver.to_string(),
    )
    .unwrap();
    assert_eq!(
        res,
        Some(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: "cw20:foobar".to_string(),
            msg: to_json_binary(&Cw20ExecuteMsg::Mint {
                recipient: receiver.to_string(),
                amount: amount_local
            })
            .unwrap(),
            funds: vec![]
        })),
    );
}

#[test]
fn test_build_burn_mapping_msg() {
    let mut deps = setup(&["channel-3", "channel-7"], &[]);
    let ibc_denom = "cosmos";
    let local_channel_id = "channel-3";
    let asset_info = AssetInfo::Token {
        contract_addr: Addr::unchecked("cw20:foobar".to_string()),
    };

    let token_factory = "token_factory".to_string();
    let contract_addr = "contract_addr".to_string();

    let amount_local = Uint128::from(10000u128);

    // case 1: on mappinglist, but mapping mechanism is not mint burn
    let mut update = UpdatePairMsg {
        local_channel_id: local_channel_id.to_string(),
        denom: ibc_denom.to_string(),
        local_asset_info: asset_info.clone(),
        remote_decimals: 18,
        local_asset_info_decimals: 18,
        is_mint_burn: None,
    };
    let msg = ExecuteMsg::UpdateMappingPair(update.clone());
    let info = mock_info("gov", &coins(1234567, "ucosm"));
    execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();
    let res = build_burn_mapping_msg(
        token_factory.clone(),
        false,
        asset_info.clone(),
        amount_local,
        contract_addr.clone(),
    );
    assert_eq!(res, Ok(None));

    // case 2: on mappinglist, is mint burn but asset info is native
    let res = build_burn_mapping_msg(
        token_factory.clone(),
        true,
        AssetInfo::NativeToken {
            denom: "orai".to_string(),
        }
        .clone(),
        amount_local,
        contract_addr.clone(),
    )
    .unwrap();
    assert_eq!(
        res,
        Some(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: token_factory.clone(),
            msg: to_json_binary(&tokenfactory::msg::ExecuteMsg::BurnTokens {
                denom: "orai".to_string(),
                amount: amount_local,
                burn_from_address: contract_addr.clone(),
            })
            .unwrap(),
            funds: vec![]
        })),
    );

    // case 3: got mint msg
    update.is_mint_burn = Some(true);
    let msg = ExecuteMsg::UpdateMappingPair(update.clone());
    let info = mock_info("gov", &coins(1234567, "ucosm"));
    execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();
    let res = build_burn_mapping_msg(
        token_factory.clone(),
        true,
        asset_info.clone(),
        amount_local,
        contract_addr.clone(),
    )
    .unwrap();
    assert_eq!(
        res,
        Some(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: "cw20:foobar".to_string(),
            msg: to_json_binary(&Cw20ExecuteMsg::Burn {
                amount: amount_local
            })
            .unwrap(),
            funds: vec![]
        })),
    );
}

#[test]
fn test_increase_channel_balance_ibc_receive_with_mint_burn() {
    let local_channel_id = "channel-0";
    let amount = Uint128::from(1_000_000_000_000_000_000u128);
    let ibc_denom = "foobar";
    let local_receiver = "receiver";
    let mut deps = setup(&[local_channel_id], &[]);
    let cw20_addr = "cw20";

    let local_asset_info = AssetInfo::Token {
        contract_addr: Addr::unchecked(cw20_addr),
    };

    let ibc_denom_keys = format!(
        "wasm.{}/{}/{}",
        mock_env().contract.address.to_string(),
        local_channel_id,
        ibc_denom
    );

    // register mapping
    let update = UpdatePairMsg {
        local_channel_id: local_channel_id.to_string(),
        denom: ibc_denom.to_string(),
        local_asset_info: local_asset_info.clone(),
        remote_decimals: 18,
        local_asset_info_decimals: 6,
        is_mint_burn: Some(true),
    };
    execute(
        deps.as_mut(),
        mock_env(),
        mock_info("gov", &vec![]),
        ExecuteMsg::UpdateMappingPair(update),
    )
    .unwrap();

    assert_eq!(
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info("attacker", &vec![]),
            ExecuteMsg::IncreaseChannelBalanceIbcReceive {
                dest_channel_id: local_channel_id.to_string(),
                ibc_denom: ibc_denom_keys.to_string(),
                amount: amount.clone(),
                local_receiver: local_receiver.to_string(),
            },
        )
        .unwrap_err(),
        ContractError::Std(StdError::generic_err("Caller is not the contract itself!"))
    );

    let res = execute(
        deps.as_mut(),
        mock_env(),
        mock_info(mock_env().contract.address.as_str(), &vec![]),
        ExecuteMsg::IncreaseChannelBalanceIbcReceive {
            dest_channel_id: local_channel_id.to_string(),
            ibc_denom: ibc_denom_keys.to_string(),
            amount: amount.clone(),
            local_receiver: local_receiver.to_string(),
        },
    )
    .unwrap();

    match res.messages[0].msg.clone() {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds: _,
        }) => {
            assert_eq!(contract_addr, cw20_addr);
            assert_eq!(
                msg,
                to_json_binary(&Cw20ExecuteMsg::Mint {
                    recipient: mock_env().contract.address.to_string(),
                    amount: convert_remote_to_local(amount, 18, 6).unwrap()
                })
                .unwrap()
            )
        }
        _ => panic!("Unexpected return message: {:?}", res.messages[0]),
    }

    let channel_state = CHANNEL_REVERSE_STATE
        .load(deps.as_ref().storage, (local_channel_id, &ibc_denom_keys))
        .unwrap();
    assert_eq!(channel_state.outstanding, amount.clone());
    assert_eq!(channel_state.total_sent, amount.clone());
    let reply_args = REPLY_ARGS.load(deps.as_ref().storage).unwrap();
    assert_eq!(reply_args.amount, amount.clone());
    assert_eq!(reply_args.channel, local_channel_id);
    assert_eq!(reply_args.denom, ibc_denom_keys.to_string());
    assert_eq!(reply_args.local_receiver, local_receiver.to_string());
}

#[test]
fn test_reduce_channel_balance_ibc_receive_with_mint_burn() {
    let local_channel_id = "channel-0";
    let amount = Uint128::from(1_000_000_000_000_000_000u128);
    let ibc_denom = "foobar";
    let local_receiver = "receiver";
    let mut deps = setup(&[local_channel_id], &[]);
    let cw20_addr = "cw20";

    let local_asset_info = AssetInfo::Token {
        contract_addr: Addr::unchecked(cw20_addr),
    };

    let ibc_denom_keys = format!(
        "wasm.{}/{}/{}",
        mock_env().contract.address.to_string(),
        local_channel_id,
        ibc_denom
    );

    // register mapping
    let update = UpdatePairMsg {
        local_channel_id: local_channel_id.to_string(),
        denom: ibc_denom.to_string(),
        local_asset_info: local_asset_info.clone(),
        remote_decimals: 18,
        local_asset_info_decimals: 6,
        is_mint_burn: Some(true),
    };
    execute(
        deps.as_mut(),
        mock_env(),
        mock_info("gov", &vec![]),
        ExecuteMsg::UpdateMappingPair(update),
    )
    .unwrap();

    execute(
        deps.as_mut(),
        mock_env(),
        mock_info(mock_env().contract.address.as_str(), &vec![]),
        ExecuteMsg::IncreaseChannelBalanceIbcReceive {
            dest_channel_id: local_channel_id.to_string(),
            ibc_denom: ibc_denom_keys.to_string(),
            amount: amount.clone(),
            local_receiver: local_receiver.to_string(),
        },
    )
    .unwrap();

    assert_eq!(
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info("attacker", &vec![]),
            ExecuteMsg::ReduceChannelBalanceIbcReceive {
                src_channel_id: local_channel_id.to_string(),
                ibc_denom: ibc_denom_keys.to_string(),
                amount: amount.clone(),
                local_receiver: local_receiver.to_string(),
            },
        )
        .unwrap_err(),
        ContractError::Std(StdError::generic_err("Caller is not the contract itself!"))
    );

    let res = execute(
        deps.as_mut(),
        mock_env(),
        mock_info(mock_env().contract.address.as_str(), &vec![]),
        ExecuteMsg::ReduceChannelBalanceIbcReceive {
            src_channel_id: local_channel_id.to_string(),
            ibc_denom: ibc_denom_keys.to_string(),
            amount: amount.clone(),
            local_receiver: local_receiver.to_string(),
        },
    )
    .unwrap();

    match res.messages[0].msg.clone() {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds: _,
        }) => {
            assert_eq!(contract_addr, cw20_addr);
            assert_eq!(
                msg,
                to_json_binary(&Cw20ExecuteMsg::Burn {
                    amount: convert_remote_to_local(amount, 18, 6).unwrap()
                })
                .unwrap()
            )
        }
        _ => panic!("Unexpected return message: {:?}", res.messages[0]),
    }

    let channel_state = CHANNEL_REVERSE_STATE
        .load(deps.as_ref().storage, (local_channel_id, &ibc_denom_keys))
        .unwrap();
    assert_eq!(channel_state.outstanding, Uint128::zero());
    assert_eq!(channel_state.total_sent, Uint128::from(amount));
    let reply_args = REPLY_ARGS.load(deps.as_ref().storage).unwrap();
    assert_eq!(reply_args.amount, amount.clone());
    assert_eq!(reply_args.channel, local_channel_id);
    assert_eq!(reply_args.denom, ibc_denom_keys);
    assert_eq!(reply_args.local_receiver, local_receiver.to_string());
}

#[test]
pub fn test_get_follow_up_msg() {
    let mut deps = mock_dependencies();
    let deps_mut = deps.as_mut();
    CONFIG
        .save(
            deps_mut.storage,
            &Config {
                default_timeout: 7600,
                default_gas_limit: None,
                fee_denom: "orai".to_string(),
                swap_router_contract: RouterController("router".to_string()),
                token_fee_receiver: Addr::unchecked("token_fee_receiver"),
                relayer_fee_receiver: Addr::unchecked("relayer_fee_receiver"),
                converter_contract: ConverterController("converter".to_string()),
                osor_entrypoint_contract: "osor_entrypoint_contract".to_string(),
                token_factory_addr: Addr::unchecked("token_factory_addr"),
            },
        )
        .unwrap();

    let orai_receiver = "orai123".to_string();
    let to_send = Amount::Cw20(Cw20CoinVerified {
        address: Addr::unchecked("cw20"),
        amount: Uint128::new(1000000),
    });

    // case 1: memo None => send only
    let msgs = get_follow_up_msgs(
        deps_mut.storage,
        deps_mut.api,
        orai_receiver.clone(),
        to_send.clone(),
        None,
    )
    .unwrap();
    assert_eq!(
        msgs,
        vec![SubMsg::reply_on_error(
            wasm_execute(
                "cw20".to_string(),
                &Cw20ExecuteMsg::Transfer {
                    recipient: orai_receiver.clone(),
                    amount: Uint128::new(1000000)
                },
                vec![]
            )
            .unwrap(),
            NATIVE_RECEIVE_ID
        ),]
    );

    // case 2: memo empty => send only
    let msgs = get_follow_up_msgs(
        deps_mut.storage,
        deps_mut.api,
        orai_receiver.clone(),
        to_send.clone(),
        Some("".to_string()),
    )
    .unwrap();
    assert_eq!(
        msgs,
        vec![SubMsg::reply_on_error(
            wasm_execute(
                "cw20".to_string(),
                &Cw20ExecuteMsg::Transfer {
                    recipient: orai_receiver.clone(),
                    amount: Uint128::new(1000000)
                },
                vec![]
            )
            .unwrap(),
            NATIVE_RECEIVE_ID
        ),]
    );

    // case 3: memo is orai_address => send_only
    let msgs = get_follow_up_msgs(
        deps_mut.storage,
        deps_mut.api,
        orai_receiver.clone(),
        to_send.clone(),
        Some(orai_receiver.to_string()),
    )
    .unwrap();
    assert_eq!(
        msgs,
        vec![SubMsg::reply_on_error(
            wasm_execute(
                "cw20".to_string(),
                &Cw20ExecuteMsg::Transfer {
                    recipient: orai_receiver.clone(),
                    amount: Uint128::new(1000000)
                },
                vec![]
            )
            .unwrap(),
            NATIVE_RECEIVE_ID
        ),]
    );
    // case 4: call universal swap (todo)
}

#[test]
fn test_withdraw_stuck_asset() {
    let mut deps = mock_dependencies();
    ADMIN
        .set(deps.as_mut(), Some(Addr::unchecked("admin")))
        .unwrap();

    // case 1: unauthorized
    let err = execute(
        deps.as_mut(),
        mock_env(),
        mock_info("addr000", &[]),
        ExecuteMsg::WithdrawAsset {
            coin: Amount::Native(Coin {
                denom: "orai".to_string(),
                amount: Uint128::new(1000000),
            }),
            receiver: Some(Addr::unchecked("receiver")),
        },
    )
    .unwrap_err();
    assert_eq!(err, ContractError::Admin(AdminError::NotAdmin {}));

    // case 2: success
    let res = execute(
        deps.as_mut(),
        mock_env(),
        mock_info("admin", &[]),
        ExecuteMsg::WithdrawAsset {
            coin: Amount::Native(Coin {
                denom: "orai".to_string(),
                amount: Uint128::new(1000000),
            }),
            receiver: Some(Addr::unchecked("receiver")),
        },
    )
    .unwrap();
    assert_eq!(
        res.messages,
        vec![SubMsg::new(CosmosMsg::Bank(BankMsg::Send {
            to_address: "receiver".to_string(),
            amount: vec![Coin {
                denom: "orai".to_string(),
                amount: Uint128::new(1000000)
            }]
        }))]
    );
}
