import {Uint128, Binary, AssetInfo, Addr, Cw20ReceiveMsg, TransferBackMsg, Coin} from "./types";
export interface InstantiateMsg {
  allowlist: AllowMsg[];
  converter_contract: string;
  default_gas_limit?: number | null;
  default_timeout: number;
  gov_contract: string;
  osor_entrypoint_contract: string;
  swap_router_contract: string;
  token_factory_addr: string;
}
export interface AllowMsg {
  contract: string;
  gas_limit?: number | null;
}
export type ExecuteMsg = {
  receive: Cw20ReceiveMsg;
} | {
  transfer_to_remote: TransferBackMsg;
} | {
  update_mapping_pair: UpdatePairMsg;
} | {
  delete_mapping_pair: DeletePairMsg;
} | {
  update_config: {
    admin?: string | null;
    converter_contract?: string | null;
    default_gas_limit?: number | null;
    default_timeout?: number | null;
    fee_receiver?: string | null;
    osor_entrypoint_contract?: string | null;
    relayer_fee?: RelayerFee[] | null;
    relayer_fee_receiver?: string | null;
    swap_router_contract?: string | null;
    token_factory_addr?: string | null;
    token_fee?: TokenFee[] | null;
  };
} | {
  increase_channel_balance_ibc_receive: {
    amount: Uint128;
    dest_channel_id: string;
    ibc_denom: string;
    local_receiver: string;
  };
} | {
  reduce_channel_balance_ibc_receive: {
    amount: Uint128;
    ibc_denom: string;
    local_receiver: string;
    src_channel_id: string;
  };
} | {
  override_channel_balance: {
    channel_id: string;
    ibc_denom: string;
    outstanding: Uint128;
    total_sent?: Uint128 | null;
  };
} | {
  ibc_hooks_receive: {
    args: Binary;
    func: HookMethods;
    orai_receiver: string;
  };
} | {
  register_denom: RegisterDenomMsg;
} | {
  withdraw_asset: {
    coin: Amount;
    receiver?: Addr | null;
  };
} | {
  clock_end_block: {
    hash: string;
  };
};
export type HookMethods = "universal_swap";
export type Amount = {
  native: Coin;
} | {
  cw20: Cw20CoinVerified;
};
export interface UpdatePairMsg {
  denom: string;
  is_mint_burn?: boolean | null;
  local_asset_info: AssetInfo;
  local_asset_info_decimals: number;
  local_channel_id: string;
  remote_decimals: number;
}
export interface DeletePairMsg {
  denom: string;
  local_channel_id: string;
}
export interface RelayerFee {
  fee: Uint128;
  prefix: string;
}
export interface TokenFee {
  ratio: Ratio;
  token_denom: string;
}
export interface Ratio {
  denominator: number;
  nominator: number;
}
export interface RegisterDenomMsg {
  metadata?: Metadata | null;
  subdenom: string;
}
export interface Metadata {
  base?: string | null;
  denom_units: DenomUnit[];
  description?: string | null;
  display?: string | null;
  name?: string | null;
  symbol?: string | null;
}
export interface DenomUnit {
  aliases: string[];
  denom: string;
  exponent: number;
}
export interface Cw20CoinVerified {
  address: Addr;
  amount: Uint128;
}
export type QueryMsg = {
  port: {};
} | {
  list_channels: {};
} | {
  channel: {
    id: string;
  };
} | {
  channel_with_key: {
    channel_id: string;
    denom: string;
  };
} | {
  config: {};
} | {
  admin: {};
} | {
  allowed: {
    contract: string;
  };
} | {
  list_allowed: {
    limit?: number | null;
    order?: number | null;
    start_after?: string | null;
  };
} | {
  pair_mappings: {
    limit?: number | null;
    order?: number | null;
    start_after?: string | null;
  };
} | {
  pair_mapping: {
    key: string;
  };
} | {
  pair_mappings_from_asset_info: {
    asset_info: AssetInfo;
  };
} | {
  get_transfer_token_fee: {
    remote_token_denom: string;
  };
} | {
  refund_info_list: {};
};
export interface AdminResponse {
  admin?: string | null;
}
export interface AllowedResponse {
  gas_limit?: number | null;
  is_allowed: boolean;
}
export interface ChannelResponse {
  balances: Amount[];
  info: ChannelInfo;
  total_sent: Amount[];
}
export interface ChannelInfo {
  connection_id: string;
  counterparty_endpoint: IbcEndpoint;
  id: string;
}
export interface IbcEndpoint {
  channel_id: string;
  port_id: string;
}
export interface ChannelWithKeyResponse {
  balance: Amount;
  info: ChannelInfo;
  total_sent: Amount;
}
export interface ConfigResponse {
  converter_contract: string;
  default_gas_limit?: number | null;
  default_timeout: number;
  fee_denom: string;
  gov_contract: string;
  osor_entrypoint_contract: string;
  relayer_fee_receiver: Addr;
  relayer_fees: RelayerFeeResponse[];
  swap_router_contract: string;
  token_fee_receiver: Addr;
  token_fees: TokenFee[];
}
export interface RelayerFeeResponse {
  amount: Uint128;
  prefix: string;
}
export interface ListAllowedResponse {
  allow: AllowedInfo[];
}
export interface AllowedInfo {
  contract: string;
  gas_limit?: number | null;
}
export interface ListChannelsResponse {
  channels: ChannelInfo[];
}
export interface PairQuery {
  key: string;
  pair_mapping: MappingMetadata;
}
export interface MappingMetadata {
  asset_info: AssetInfo;
  asset_info_decimals: number;
  is_mint_burn?: boolean;
  remote_decimals: number;
}
export type ArrayOfPairQuery = PairQuery[];
export interface PortResponse {
  port_id: string;
}
export type ArrayOfRefundInfo = RefundInfo[];
export interface RefundInfo {
  amount: Amount;
  receiver: string;
}