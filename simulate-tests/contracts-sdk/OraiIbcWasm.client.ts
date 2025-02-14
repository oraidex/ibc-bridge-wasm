/**
* This file was automatically generated by @oraichain/ts-codegen@0.35.9.
* DO NOT MODIFY IT BY HAND. Instead, modify the source JSONSchema file,
* and run the @oraichain/ts-codegen generate command to regenerate this file.
*/

import { CosmWasmClient, SigningCosmWasmClient, ExecuteResult } from "@cosmjs/cosmwasm-stargate";
import { StdFee } from "@cosmjs/amino";
import {Asset, Uint128, Binary, Addr, Coin, Cw20Coin, TransferBackMsg, Cw20ReceiveMsg} from "./types";
import {InstantiateMsg, ExecuteMsg, QueryMsg} from "./OraiIbcWasm.types";
export interface OraiIbcWasmReadOnlyInterface {
  contractAddress: string;
}
export class OraiIbcWasmQueryClient implements OraiIbcWasmReadOnlyInterface {
  client: CosmWasmClient;
  contractAddress: string;

  constructor(client: CosmWasmClient, contractAddress: string) {
    this.client = client;
    this.contractAddress = contractAddress;
  }

}
export interface OraiIbcWasmInterface extends OraiIbcWasmReadOnlyInterface {
  contractAddress: string;
  sender: string;
  ibcWasmTransfer: ({
    coin,
    ibcWasmInfo
  }: {
    coin: Asset;
    ibcWasmInfo: TransferBackMsg;
  }, _fee?: number | StdFee | "auto", _memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  receive: ({
    amount,
    msg,
    sender
  }: {
    amount: Uint128;
    msg: Binary;
    sender: string;
  }, _fee?: number | StdFee | "auto", _memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  updateOwner: ({
    newOwner
  }: {
    newOwner: Addr;
  }, _fee?: number | StdFee | "auto", _memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  withdrawAsset: ({
    coin,
    receiver
  }: {
    coin: Asset;
    receiver?: Addr;
  }, _fee?: number | StdFee | "auto", _memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
}
export class OraiIbcWasmClient extends OraiIbcWasmQueryClient implements OraiIbcWasmInterface {
  client: SigningCosmWasmClient;
  sender: string;
  contractAddress: string;

  constructor(client: SigningCosmWasmClient, sender: string, contractAddress: string) {
    super(client, contractAddress);
    this.client = client;
    this.sender = sender;
    this.contractAddress = contractAddress;
    this.ibcWasmTransfer = this.ibcWasmTransfer.bind(this);
    this.receive = this.receive.bind(this);
    this.updateOwner = this.updateOwner.bind(this);
    this.withdrawAsset = this.withdrawAsset.bind(this);
  }

  ibcWasmTransfer = async ({
    coin,
    ibcWasmInfo
  }: {
    coin: Asset;
    ibcWasmInfo: TransferBackMsg;
  }, _fee: number | StdFee | "auto" = "auto", _memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      ibc_wasm_transfer: {
        coin,
        ibc_wasm_info: ibcWasmInfo
      }
    }, _fee, _memo, _funds);
  };
  receive = async ({
    amount,
    msg,
    sender
  }: {
    amount: Uint128;
    msg: Binary;
    sender: string;
  }, _fee: number | StdFee | "auto" = "auto", _memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      receive: {
        amount,
        msg,
        sender
      }
    }, _fee, _memo, _funds);
  };
  updateOwner = async ({
    newOwner
  }: {
    newOwner: Addr;
  }, _fee: number | StdFee | "auto" = "auto", _memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      update_owner: {
        new_owner: newOwner
      }
    }, _fee, _memo, _funds);
  };
  withdrawAsset = async ({
    coin,
    receiver
  }: {
    coin: Asset;
    receiver?: Addr;
  }, _fee: number | StdFee | "auto" = "auto", _memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      withdraw_asset: {
        coin,
        receiver
      }
    }, _fee, _memo, _funds);
  };
}