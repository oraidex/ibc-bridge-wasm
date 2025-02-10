import { SimulateCosmWasmClient } from "@oraichain/cw-simulate";
import {
  AssetInfo,
  OraiswapFactoryClient,
  OraiswapOracleClient,
  OraiswapRouterClient,
  OraiswapTokenClient,
  OraiswapV3Client,
} from "@oraichain/oraidex-contracts-sdk";
// import { CwIcs20LatestClient } from "@oraichain/common-contracts-sdk";
import { CwIcs20LatestClient } from "./contracts-sdk/CwIcs20Latest.client";
import * as oraidexArtifacts from "@oraichain/oraidex-contracts-build";
import * as commonArtifacts from "@oraichain/common-contracts-build";
import { readFileSync } from "fs";
import { Cw20Coin } from "@oraichain/common-contracts-sdk";
import "dotenv/config";
import { InstantiateMsg as CwIcs20LatestInstantiateMsg } from "./contracts-sdk/CwIcs20Latest.types";
import { InstantiateMsg as OraiSwapTokenInstantiateMsg } from "@oraichain/oraidex-contracts-sdk/build/OraiswapToken.types";
import {
  EntryPointClient,
  IbcHooksClient,
  OraidexClient,
  OraiIbcWasmClient,
  OraiswapMixedRouterClient,
} from "./contracts-sdk";
import { InstantiateMsg as OraiDexAdapterInstantiateMsg } from "./contracts-sdk/Oraidex.types";
import { InstantiateMsg as IbcWasmAdapterInstantiateMsg } from "./contracts-sdk/OraiIbcWasm.types";
import { InstantiateMsg as IbcHooksInstantiateMsg } from "./contracts-sdk/IbcHooks.types";
import { InstantiateMsg as OsorEntrypointInstantiateMsg } from "./contracts-sdk/EntryPoint.types";
import { InstantiateMsg as MixedRouterInstantiateMsg } from "./contracts-sdk/OraiswapMixedRouter.types";
import { InstantiateMsg as OraiSwapV3InstantiateMsg } from "@oraichain/oraidex-contracts-sdk/build/OraiswapV3.types";
import { Event } from "@cosmjs/stargate";

export const senderAddress = "orai1g4h64yjt0fvzv5v2j8tyfnpe5kmnetejvfgs7g";
export const tokenfactoryAddress = "orai1g4h64yjt0fvzv5v2j8tyfnpe5kmnetejvfgs7g";
export const AtomDenom =
  "ibc/A2E2EEC9057A4A1C2C0A6A4C78B0239118DF5F278830F50B4A6BDD7A66506B78";

export const deployToken = async (
  client: SimulateCosmWasmClient,
  {
    symbol,
    name,
    decimals = 6,
    initial_balances = [{ address: senderAddress, amount: "1000000000" }],
    minter = senderAddress,
  }: {
    symbol: string;
    name: string;
    decimals?: number;
    initial_balances?: Cw20Coin[];
    minter?: string;
  }
): Promise<OraiswapTokenClient> => {
  return new OraiswapTokenClient(
    client,
    senderAddress,
    (
      await oraidexArtifacts.deployContract(
        client,
        senderAddress,

        {
          decimals,
          symbol,
          name,
          mint: { minter: minter },
          initial_balances,
        } as OraiSwapTokenInstantiateMsg,
        "token",
        "oraiswap-token"
      )
    ).contractAddress
  );
};

export const deployOraiswapV3 = async (
  client: SimulateCosmWasmClient,
  {
    protocol_fee,
  }: {
    protocol_fee: number;
  }
): Promise<OraiswapV3Client> => {
  return new OraiswapV3Client(
    client,
    senderAddress,
    (
      await oraidexArtifacts.deployContract(
        client,
        senderAddress,

        {
          protocol_fee,
        } as OraiSwapV3InstantiateMsg,
        "oraiswap-v3",
        "oraiswap-v3"
      )
    ).contractAddress
  );
};

export const deployIbcWasmContract = async (
  client: SimulateCosmWasmClient,
  {
    swap_router_contract,
    converter_contract,
    gov_contract = senderAddress,
    token_factory_addr = tokenfactoryAddress,
    osor_entrypoint_contract,
    token_factory_addr = senderAddress,
  }: {
    gov_contract?: string;
    token_factory_addr?: string
    swap_router_contract: string;
    converter_contract: string;
    osor_entrypoint_contract: string;
    token_factory_addr?: string;
  }
): Promise<CwIcs20LatestClient> => {
  const { codeId } = await client.upload(
    senderAddress,
    readFileSync(
      process.env.ICS20_LATEST ||
        commonArtifacts.getContractDir("cw-ics20-latest")
    ),
    "auto"
  );
  const { contractAddress } = await client.instantiate(
    senderAddress,
    codeId,
    {
      allowlist: [],
      default_timeout: 3600,
      gov_contract,
      swap_router_contract,
      converter_contract,
      osor_entrypoint_contract,
      token_factory_addr,
    } as CwIcs20LatestInstantiateMsg,
    "cw-ics20-latest",
    "auto"
  );
  return new CwIcs20LatestClient(client, senderAddress, contractAddress);
};

export const deployOraiDexAdapterContract = async (
  client: SimulateCosmWasmClient,
  {
    osor_entrypoint_contract,
    oraidex_router_contract_address,
  }: {
    osor_entrypoint_contract: string;
    oraidex_router_contract_address: string;
  }
): Promise<OraidexClient> => {
  const { codeId } = await client.upload(
    senderAddress,
    readFileSync(process.env.ORAIDEX_ADAPTER!),
    "auto"
  );
  const { contractAddress } = await client.instantiate(
    senderAddress,
    codeId,
    {
      entry_point_contract_address: osor_entrypoint_contract,
      oraidex_router_contract_address,
    } as OraiDexAdapterInstantiateMsg,
    "oraidex-adapter",
    "auto"
  );
  return new OraidexClient(client, senderAddress, contractAddress);
};

export const deployIbcWasmAdapterContract = async (
  client: SimulateCosmWasmClient,
  {
    osor_entrypoint_contract,
    ibc_wasm_contract_address,
  }: {
    osor_entrypoint_contract: string;
    ibc_wasm_contract_address: string;
  }
): Promise<OraiIbcWasmClient> => {
  const { codeId } = await client.upload(
    senderAddress,
    readFileSync(process.env.IBC_WASM_ADAPTER!),
    "auto"
  );
  const { contractAddress } = await client.instantiate(
    senderAddress,
    codeId,
    {
      entry_point_contract_address: osor_entrypoint_contract,
      ibc_wasm_contract_address,
    } as IbcWasmAdapterInstantiateMsg,
    "ibc-wasm-adapter",
    "auto"
  );
  return new OraiIbcWasmClient(client, senderAddress, contractAddress);
};

export const deployIbcHooksAdapterContract = async (
  client: SimulateCosmWasmClient,
  {
    osor_entrypoint_contract,
  }: {
    osor_entrypoint_contract: string;
  }
): Promise<IbcHooksClient> => {
  const { codeId } = await client.upload(
    senderAddress,
    readFileSync(process.env.IBC_HOOKS_ADAPTER!),
    "auto"
  );
  const { contractAddress } = await client.instantiate(
    senderAddress,
    codeId,
    {
      entry_point_contract_address: osor_entrypoint_contract,
    } as IbcHooksInstantiateMsg,
    "ibc-hooks-adapter",
    "auto"
  );
  return new IbcHooksClient(client, senderAddress, contractAddress);
};

export const deployOsorEntrypointContract = async (
  client: SimulateCosmWasmClient
): Promise<EntryPointClient> => {
  const { codeId } = await client.upload(
    senderAddress,
    readFileSync(process.env.OSOR_ENTRYPOINT!),
    "auto"
  );
  const { contractAddress } = await client.instantiate(
    senderAddress,
    codeId,
    {
      swap_venues: [],
    } as OsorEntrypointInstantiateMsg,
    "osor-entrypoint",
    "auto"
  );
  return new EntryPointClient(client, senderAddress, contractAddress);
};

export const deployMixedRouterContract = async (
  client: SimulateCosmWasmClient,
  {
    factory_addr,
    oraiswap_v3,
  }: {
    factory_addr: string;
    oraiswap_v3: string;
  }
): Promise<OraiswapMixedRouterClient> => {
  const { codeId } = await client.upload(
    senderAddress,
    readFileSync(process.env.MIXED_ROUTER!),
    "auto"
  );
  const { contractAddress } = await client.instantiate(
    senderAddress,
    codeId,
    {
      factory_addr,
      factory_addr_v2: factory_addr,
      oraiswap_v3,
    } as MixedRouterInstantiateMsg,
    "mixed-router",
    "auto"
  );
  return new OraiswapMixedRouterClient(client, senderAddress, contractAddress);
};

export const deployV2Contracts = async (
  oraiClient: SimulateCosmWasmClient,
  ibcWasmContractAddress: string,
  initialBalanceAmount: string,
  oraiSenderAddress = senderAddress
) => {
  let factoryContract: OraiswapFactoryClient;
  let routerContract: OraiswapRouterClient;
  let usdtToken: OraiswapTokenClient;
  let oracleContract: OraiswapOracleClient;

  // upload pair & lp token code id
  const { codeId: pairCodeId } = await oraiClient.upload(
    oraiSenderAddress,
    readFileSync(oraidexArtifacts.getContractDir("oraiswap-pair")),
    "auto"
  );
  const { codeId: lpCodeId } = await oraiClient.upload(
    oraiSenderAddress,
    readFileSync(oraidexArtifacts.getContractDir("oraiswap-token")),
    "auto"
  );
  // deploy another cw20 for oraiswap testing
  const { contractAddress: usdtAddress } = await oraiClient.instantiate(
    oraiSenderAddress,
    lpCodeId,
    {
      decimals: 6,
      symbol: "USDT",
      name: "USDT token",
      initial_balances: [
        {
          address: ibcWasmContractAddress,
          amount: initialBalanceAmount,
        },
      ],
      mint: {
        minter: oraiSenderAddress,
      },
    },
    "cw20-usdt"
  );
  usdtToken = new OraiswapTokenClient(
    oraiClient,
    oraiSenderAddress,
    usdtAddress
  );
  // deploy oracle addr
  const { contractAddress: oracleAddress } =
    await oraidexArtifacts.deployContract(
      oraiClient,
      oraiSenderAddress,
      {},
      "oraiswap-oracle",
      "oraiswap-oracle"
    );
  // deploy factory contract
  oracleContract = new OraiswapOracleClient(
    oraiClient,
    oraiSenderAddress,
    oracleAddress
  );

  await oracleContract.updateTaxRate({ rate: "0" });
  await oracleContract.updateTaxCap({ denom: AtomDenom, cap: "100000" });
  const { contractAddress: factoryAddress } =
    await oraidexArtifacts.deployContract(
      oraiClient,
      oraiSenderAddress,
      {
        commission_rate: "0",
        oracle_addr: oracleAddress,
        pair_code_id: pairCodeId,
        token_code_id: lpCodeId,
      },
      "oraiswap-factory",
      "oraiswap-factory"
    );

  const { contractAddress: routerAddress } =
    await oraidexArtifacts.deployContract(
      oraiClient,
      oraiSenderAddress,
      {
        factory_addr: factoryAddress,
        factory_addr_v2: factoryAddress,
      },
      "oraiswap-router",
      "oraiswap-router"
    );
  factoryContract = new OraiswapFactoryClient(
    oraiClient,
    oraiSenderAddress,
    factoryAddress
  );
  routerContract = new OraiswapRouterClient(
    oraiClient,
    oraiSenderAddress,
    routerAddress
  );
  return {
    factoryContract,
    oracleContract,
    usdtToken,
    routerContract,
    lpCodeId,
  };
};
