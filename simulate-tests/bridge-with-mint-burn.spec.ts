import { Event, toBinary } from "@cosmjs/cosmwasm-stargate";
import { Coin, coins, coin } from "@cosmjs/proto-signing";
import {
  CWSimulateApp,
  GenericError,
  IbcOrder,
  IbcPacket,
  SimulateCosmWasmClient,
} from "@oraichain/cw-simulate";
import bech32 from "bech32";
import {
  OraiswapFactoryClient,
  OraiswapRouterClient,
  OraiswapTokenClient,
  OraiswapPairClient,
  OraiswapOracleClient,
  OraiswapV3Client,
  PairInfo,
} from "@oraichain/oraidex-contracts-sdk";
// import { CwIcs20LatestClient } from "@oraichain/common-contracts-sdk";
import { CwIcs20LatestClient } from "./contracts-sdk/CwIcs20Latest.client";
import { FungibleTokenPacketData } from "cosmjs-types/ibc/applications/transfer/v2/packet";
import {
  deployIbcHooksAdapterContract,
  deployIbcWasmAdapterContract,
  deployIbcWasmContract,
  deployMixedRouterContract,
  deployOraiDexAdapterContract,
  deployOraiswapV3,
  deployOsorEntrypointContract,
  deployToken,
  deployV2Contracts,
  senderAddress as oraiSenderAddress,
  senderAddress,
} from "./common";
import { oraib2oraichain, toAmount } from "@oraichain/oraidex-common";
import { ORAI } from "@oraichain/oraidex-common";
import { toDisplay } from "@oraichain/oraidex-common";
import { expect, afterAll, beforeAll, describe, it, beforeEach } from "vitest";
import {
  OraiswapMixedRouterClient,
  EntryPointClient,
  OraidexClient,
  OraiIbcWasmClient,
  IbcHooksClient,
} from "./contracts-sdk";
import {
  buildUniversalSwapMemo,
  SWAP_VENUE_NAME,
} from "@oraichain/oraidex-universal-swap";

let cosmosChain: CWSimulateApp;
// oraichain support cosmwasm
let oraiClient: SimulateCosmWasmClient;

const bobAddress = "orai1ur2vsjrjarygawpdwtqteaazfchvw4fg6uql76";
const bobAddressEth = "0x8754032Ac7966A909e2E753308dF56bb08DabD69";
const bridgeReceiver = "tron-testnet0x3C5C6b570C1DA469E8B24A2E8Ed33c278bDA3222";
const routerContractAddress = "placeholder"; // we will update the contract config later when we need to deploy the actual router contract
const converterContractAddress = "converter"; // we will update the contract config later when we need to deploy the actual converter contract
const osorEntrypointContractAddressTemp = "osor"; // we will update the contract config later when we need to deploy the actual converter contract
const cosmosSenderAddress = bech32.encode(
  "cosmos",
  bech32.decode(oraiSenderAddress).words
);
const relayerAddress = "orai1704r4dhuwdqvt7vs35m0360py6ep6cwwxeyfxn";
const oraibridgeSenderAddress = bech32.encode(
  "oraib",
  bech32.decode(oraiSenderAddress).words
);
console.log({ cosmosSenderAddress });
const ibcTransferAmount = "100000000";
const initialBalanceAmount = "10000000000000";

describe.only("IBCModuleWithMintBurn", () => {
  let oraiPort: string;
  let oraiIbcDenom: string =
    "tron-testnet0xA325Ad6D9c92B55A3Fc5aD7e412B1518F96441C0";
  let airiIbcDenom: string =
    "tron-testnet0x7e2A35C746F2f7C240B664F1Da4DD100141AE71F";
  let usdtIbcDenom: string =
    "tron-testnet0xdac17f958d2ee523a2206206994597c13d831ec7";
  let AtomDenom =
    "ibc/A2E2EEC9057A4A1C2C0A6A4C78B0239118DF5F278830F50B4A6BDD7A66506B78";
  let atomChannel = "channel-15";
  let cosmosPort: string = "transfer";
  let channel = "channel-0";
  let ics20Contract: CwIcs20LatestClient;
  let oraiswapV3: OraiswapV3Client;
  let mixedRouterContract: OraiswapMixedRouterClient;
  let osorEntrypointContract: EntryPointClient;
  let oraidexAdapterContract: OraidexClient;
  let ibcWasmAdapterContract: OraiIbcWasmClient;
  let ibcHooksAdapterContract: IbcHooksClient;
  let factoryContract: OraiswapFactoryClient;
  let routerContract: OraiswapRouterClient;
  let usdtToken: OraiswapTokenClient;
  let oracleContract: OraiswapOracleClient;
  let airiToken: OraiswapTokenClient;
  let lpId: number;

  let packetData = {
    src: {
      port_id: cosmosPort,
      channel_id: channel,
    },
    dest: {
      port_id: oraiPort,
      channel_id: channel,
    },
    sequence: 27,
    timeout: {
      block: {
        revision: 1,
        height: 12345678,
      },
    },
  };
  beforeEach(async () => {
    // reset state for every test
    cosmosChain = new CWSimulateApp({
      chainId: "cosmoshub-4",
      bech32Prefix: "cosmos",
    });

    oraiClient = new SimulateCosmWasmClient({
      chainId: "Oraichain",
      bech32Prefix: ORAI,
      metering: process.env.METERING === "true",
    });

    ics20Contract = await deployIbcWasmContract(oraiClient, {
      swap_router_contract: routerContractAddress,
      converter_contract: converterContractAddress,
      osor_entrypoint_contract: osorEntrypointContractAddressTemp,
    });

    oraiswapV3 = await deployOraiswapV3(oraiClient, { protocol_fee: 1000 });

    const result = await deployV2Contracts(
      oraiClient,
      ics20Contract.contractAddress,
      initialBalanceAmount,
      senderAddress
    );
    lpId = result.lpCodeId;
    factoryContract = result.factoryContract;
    oracleContract = result.oracleContract;
    routerContract = result.routerContract;
    usdtToken = result.usdtToken;
    mixedRouterContract = await deployMixedRouterContract(oraiClient, {
      factory_addr: factoryContract.contractAddress,
      oraiswap_v3: oraiswapV3.contractAddress,
    });

    osorEntrypointContract = await deployOsorEntrypointContract(oraiClient);

    oraidexAdapterContract = await deployOraiDexAdapterContract(oraiClient, {
      osor_entrypoint_contract: osorEntrypointContract.contractAddress,
      oraidex_router_contract_address: mixedRouterContract.contractAddress,
    });

    ibcWasmAdapterContract = await deployIbcWasmAdapterContract(oraiClient, {
      osor_entrypoint_contract: osorEntrypointContract.contractAddress,
      ibc_wasm_contract_address: ics20Contract.contractAddress,
    });

    ibcHooksAdapterContract = await deployIbcHooksAdapterContract(oraiClient, {
      osor_entrypoint_contract: osorEntrypointContract.contractAddress,
    });

    await ics20Contract.updateConfig({
      osorEntrypointContract: osorEntrypointContract.contractAddress,
      swapRouterContract: mixedRouterContract.contractAddress,
    });

    // add swap venue for entrypoint to universal swap
    await osorEntrypointContract.updateConfig({
      swapVenues: [
        {
          name: SWAP_VENUE_NAME,
          adapter_contract_address: oraidexAdapterContract.contractAddress,
        },
      ],
      ibcTransferContractAddress: ibcHooksAdapterContract.contractAddress,
      ibcWasmContractAddress: ibcWasmAdapterContract.contractAddress,
    });

    oraiPort = "wasm." + ics20Contract.contractAddress;
    packetData.dest.port_id = oraiPort;

    // init cw20 AIRI token
    airiToken = await deployToken(oraiClient, {
      decimals: 6,
      symbol: "AIRI",
      name: "Airight token",
      initial_balances: [],
      minter: ics20Contract.contractAddress,
    });

    // init ibc channel between two chains
    oraiClient.app.ibc.relay(
      channel,
      oraiPort,
      channel,
      cosmosPort,
      cosmosChain
    );
    await cosmosChain.ibc.sendChannelOpen({
      open_init: {
        channel: {
          counterparty_endpoint: {
            port_id: oraiPort,
            channel_id: channel,
          },
          endpoint: {
            port_id: cosmosPort,
            channel_id: channel,
          },
          order: IbcOrder.Unordered,
          version: "ics20-1",
          connection_id: "connection-0",
        },
      },
    });

    await cosmosChain.ibc.sendChannelConnect({
      open_ack: {
        channel: {
          counterparty_endpoint: {
            port_id: oraiPort,
            channel_id: channel,
          },
          endpoint: {
            port_id: cosmosPort,
            channel_id: channel,
          },
          order: IbcOrder.Unordered,
          version: "ics20-1",
          connection_id: "connection-0",
        },
        counterparty_version: "ics20-1",
      },
    });

    cosmosChain.ibc.addMiddleWare((msg, app) => {
      const data = msg.data.packet as IbcPacket;
      if (Number(data.timeout.timestamp) < cosmosChain.time) {
        throw new GenericError("timeout at " + data.timeout.timestamp);
      }
    });
    // topup
    oraiClient.app.bank.setBalance(
      ics20Contract.contractAddress,
      coins(initialBalanceAmount, ORAI)
    );

    await ics20Contract.updateMappingPair({
      localAssetInfo: {
        token: {
          contract_addr: airiToken.contractAddress,
        },
      },
      localAssetInfoDecimals: 6,
      denom: airiIbcDenom,
      remoteDecimals: 6,
      localChannelId: channel,
      isMintBurn: true,
    });
  });

  it("mint-burn-demo-getting-channel-state-ibc-wasm-should-increase-balances-and-total-sent", async () => {
    // fixture. Setup everything from the ics 20 contract to ibc relayer
    const oraiClient = new SimulateCosmWasmClient({
      chainId: "Oraichain",
      bech32Prefix: ORAI,
      metering: process.env.METERING === "true",
    });

    const ics20Contract = await deployIbcWasmContract(oraiClient, {
      swap_router_contract: routerContractAddress,
      converter_contract: converterContractAddress,
      osor_entrypoint_contract: osorEntrypointContract.contractAddress,
    });
    const oraiPort = "wasm." + ics20Contract.contractAddress;
    let newPacketData = {
      src: {
        port_id: cosmosPort,
        channel_id: channel,
      },
      dest: {
        port_id: oraiPort,
        channel_id: channel,
      },
      sequence: 27,
      timeout: {
        block: {
          revision: 1,
          height: 12345678,
        },
      },
    };
    newPacketData.dest.port_id = oraiPort;

    // init cw20 AIRI token
    const airiToken = await deployToken(oraiClient, {
      decimals: 6,
      symbol: "AIRI",
      name: "Airight token",
      initial_balances: [
        {
          address: ics20Contract.contractAddress,
          amount: initialBalanceAmount,
        },
      ],
    });

    // init ibc channel between two chains
    oraiClient.app.ibc.relay(
      channel,
      oraiPort,
      channel,
      cosmosPort,
      cosmosChain
    );
    await cosmosChain.ibc.sendChannelOpen({
      open_init: {
        channel: {
          counterparty_endpoint: {
            port_id: oraiPort,
            channel_id: channel,
          },
          endpoint: {
            port_id: cosmosPort,
            channel_id: channel,
          },
          order: IbcOrder.Unordered,
          version: "ics20-1",
          connection_id: "connection-0",
        },
      },
    });

    await cosmosChain.ibc.sendChannelConnect({
      open_ack: {
        channel: {
          counterparty_endpoint: {
            port_id: oraiPort,
            channel_id: channel,
          },
          endpoint: {
            port_id: cosmosPort,
            channel_id: channel,
          },
          order: IbcOrder.Unordered,
          version: "ics20-1",
          connection_id: "connection-0",
        },
        counterparty_version: "ics20-1",
      },
    });

    cosmosChain.ibc.addMiddleWare((msg, app) => {
      const data = msg.data.packet as IbcPacket;
      if (Number(data.timeout.timestamp) < cosmosChain.time) {
        throw new GenericError("timeout at " + data.timeout.timestamp);
      }
    });
    // topup
    await ics20Contract.updateMappingPair({
      localAssetInfo: {
        token: {
          contract_addr: airiToken.contractAddress,
        },
      },
      localAssetInfoDecimals: 6,
      denom: airiIbcDenom,
      remoteDecimals: 6,
      localChannelId: channel,
    });

    const icsPackage: FungibleTokenPacketData = {
      amount: ibcTransferAmount,
      denom: airiIbcDenom,
      receiver: bobAddress,
      sender: cosmosSenderAddress,
      memo: "",
    };
    // transfer from cosmos to oraichain, should pass. This should increase the balances & total sent
    await cosmosChain.ibc.sendPacketReceive({
      packet: {
        data: toBinary(icsPackage),
        ...newPacketData,
      },
      relayer: relayerAddress,
    });

    const { channels } = await ics20Contract.listChannels();
    for (let channel of channels) {
      const { balances } = await ics20Contract.channel({ id: channel.id });
      console.log(balances);
      for (let balance of balances) {
        if ("native" in balance) {
          const pairMapping = await ics20Contract.pairMapping({
            key: balance.native.denom,
          });
          const { balance: channelBalance } =
            await ics20Contract.channelWithKey({
              channelId: channel.id,
              denom: balance.native.denom,
            });
          if ("native" in channelBalance) {
            const trueBalance = toDisplay(
              channelBalance.native.amount,
              pairMapping.pair_mapping.remote_decimals
            );
            expect(trueBalance).toEqual(
              parseInt(ibcTransferAmount) /
                10 ** pairMapping.pair_mapping.remote_decimals
            );
          }
        } else {
          // do nothing because currently we dont have any cw20 balance in the channel
        }
      }
    }
  });

  // // TODO: test with native_token
  // it.each([
  //   [
  //     false,
  //     {
  //       native_token: {
  //         denom: ORAI,
  //       },
  //     },
  //     ibcTransferAmount,
  //     oraiIbcDenom,
  //     coins(ibcTransferAmount, ORAI),
  //     "cw-ics20-success-should-increase-native-balance-remote-to-local",
  //   ],
  //   [
  //     false,
  //     null,
  //     ibcTransferAmount,
  //     oraiIbcDenom,
  //     [],
  //     "cw-ics20-fail-no-pair-mapping-should-not-send-balance-remote-to-local",
  //   ],
  //   [
  //     false,
  //     {
  //       native_token: {
  //         denom: ORAI,
  //       },
  //     },
  //     "10000000000001",
  //     oraiIbcDenom,
  //     [],
  //     "cw-ics20-fail-transfer-native-fail-insufficient-funds-should-not-send-balance-remote-to-local",
  //   ],
  //   [
  //     true,
  //     {
  //       token: {
  //         contract_addr: "orai18cvw806fj5n7xxz06ak8vjunveeks4zzzn37cu", // has to hard-code address airi due to jest issue: https://github.com/facebook/jest/issues/6888
  //       },
  //     },
  //     ibcTransferAmount,
  //     airiIbcDenom,
  //     [{ amount: ibcTransferAmount, denom: "" }],
  //     "cw-ics20-success-transfer-cw20-should-increase-cw20-balance-remote-to-local",
  //   ],
  // ])(
  //   "mint-burn-bridge-test-cw-ics20-transfer-remote-to-local-given %j %s %s should return expected amount %j", //reference: https://jestjs.io/docs/api#1-testeachtablename-fn-timeout
  //   async (
  //     isMintBurn: boolean,
  //     assetInfo: AssetInfo,
  //     transferAmount: string,
  //     transferDenom: string,
  //     expectedBalance: Coin[],
  //     _name: string
  //   ) => {
  //     // create mapping
  //     if (assetInfo) {
  //       const pair = {
  //         localAssetInfo: assetInfo,
  //         localAssetInfoDecimals: 6,
  //         denom: transferDenom,
  //         remoteDecimals: 6,
  //         localChannelId: channel,
  //         isMintBurn,
  //       };
  //       await ics20Contract.updateMappingPair(pair);
  //     }
  //     const icsPackage: FungibleTokenPacketData = {
  //       amount: transferAmount,
  //       denom: transferDenom,
  //       receiver: bobAddress,
  //       sender: cosmosSenderAddress,
  //       memo: "",
  //     };
  //     await cosmosChain.ibc.sendPacketReceive({
  //       packet: {
  //         data: toBinary(icsPackage),
  //         ...packetData,
  //       },
  //       relayer: relayerAddress,
  //     });

  //     if (assetInfo && (assetInfo as any).token) {
  //       const bobBalance = await airiToken.balance({ address: bobAddress });
  //       console.log("bob balance contract address: ", bobBalance);
  //       expect(bobBalance.balance).toEqual(expectedBalance[0].amount);
  //       return;
  //     }
  //     const bobBalance = oraiClient.app.bank.getBalance(icsPackage.receiver);
  //     expect(bobBalance).toMatchObject(expectedBalance);
  //   }
  // );

  // it("mint-burn-cw-ics20-success-cw20-should-transfer-balance-to-ibc-wasm-contract-local-to-remote", async () => {
  //   let ibcWasmAiriBalance = await airiToken.balance({
  //     address: ics20Contract.contractAddress,
  //   });
  //   expect(ibcWasmAiriBalance.balance).toEqual("0");
  //   // now send ibc package
  //   const icsPackage: FungibleTokenPacketData = {
  //     amount: ibcTransferAmount,
  //     denom: airiIbcDenom,
  //     receiver: bobAddress,
  //     sender: cosmosSenderAddress,
  //     memo: "",
  //   };
  //   // transfer from cosmos to oraichain, should pass
  //   await cosmosChain.ibc.sendPacketReceive({
  //     packet: {
  //       data: toBinary(icsPackage),
  //       ...packetData,
  //     },
  //     relayer: relayerAddress,
  //   });

  //   const transferBackMsg: TransferBackMsg = {
  //     local_channel_id: channel,
  //     remote_address: cosmosSenderAddress,
  //     remote_denom: airiIbcDenom,
  //   };
  //   airiToken.sender = bobAddress;
  //   await airiToken.send({
  //     amount: ibcTransferAmount,
  //     contract: ics20Contract.contractAddress,
  //     msg: Buffer.from(JSON.stringify(transferBackMsg)).toString("base64"),
  //   });
  //   ibcWasmAiriBalance = await airiToken.balance({
  //     address: ics20Contract.contractAddress,
  //   });
  //   expect(ibcWasmAiriBalance.balance).toEqual("0");
  // });

  // it.each([
  //   [
  //     "",
  //     ibcTransferAmount,
  //     "empty-memo-should-fallback-to-transfer-to-receiver",
  //   ],
  //   [
  //     buildUniversalSwapMemo(
  //       { minimumReceive: "1000000", recoveryAddr: bobAddress },
  //       { returnAmount: "10", routes: [], swapAmount: "100" }
  //     ),
  //     ibcTransferAmount,
  //     "only-receiver-memo-should-fallback-to-transfer-to-receiver",
  //   ],
  // ])(
  //   "mint-burn-cw-ics20-test-single-step-invalid-dest-denom-memo-remote-to-local-given %s should-get-expected-amount %s",
  //   async (memo: string, expectedAmount: string, _name: string) => {
  //     // now send ibc package
  //     const icsPackage: FungibleTokenPacketData = {
  //       amount: ibcTransferAmount,
  //       denom: airiIbcDenom,
  //       receiver: bobAddress,
  //       sender: cosmosSenderAddress,
  //       memo,
  //     };
  //     // transfer from cosmos to oraichain, should pass
  //     const result = await cosmosChain.ibc.sendPacketReceive({
  //       packet: {
  //         data: toBinary(icsPackage),
  //         ...packetData,
  //       },
  //       relayer: relayerAddress,
  //     });
  //     const ibcWasmAiriBalance = await airiToken.balance({
  //       address: bobAddress,
  //     });
  //     console.dir(result, { depth: null });
  //     expect(ibcWasmAiriBalance.balance).toEqual(expectedAmount);
  //   }
  // );

  // describe("mint-burn-cw-ics20-test-single-step-swap-to-tokens", () => {
  //   let factoryContract: OraiswapFactoryClient;
  //   let routerContract: OraiswapRouterClient;
  //   let airiOraiPair: PairInfo;
  //   let usdtToken: OraiswapTokenClient;
  //   let oracleContract: OraiswapOracleClient;
  //   let assetInfos: AssetInfo[];
  //   let lpId: number;
  //   let icsPackage: FungibleTokenPacketData = {
  //     amount: ibcTransferAmount,
  //     denom: airiIbcDenom,
  //     receiver: bobAddress,
  //     sender: cosmosSenderAddress,
  //     memo: "",
  //   };
  //   const findWasmEvent = (events: Event[], key: string, value: string) =>
  //     events.find(
  //       (event) =>
  //         event.type === "wasm" &&
  //         event.attributes.find(
  //           (attr) => attr.key === key && attr.value === value
  //         )
  //     );
  //   beforeEach(async () => {
  //     assetInfos = [
  //       { native_token: { denom: ORAI } },
  //       { token: { contract_addr: airiToken.contractAddress } },
  //     ];
  //     // upload pair & lp token code id
  //     const { codeId: pairCodeId } = await oraiClient.upload(
  //       oraiSenderAddress,
  //       readFileSync(oraidexArtifacts.getContractDir("oraiswap-pair")),
  //       "auto"
  //     );
  //     const { codeId: lpCodeId } = await oraiClient.upload(
  //       oraiSenderAddress,
  //       readFileSync(oraidexArtifacts.getContractDir("oraiswap-token")),
  //       "auto"
  //     );
  //     lpId = lpCodeId;
  //     // deploy another cw20 for oraiswap testing
  //     const { contractAddress: usdtAddress } = await oraiClient.instantiate(
  //       oraiSenderAddress,
  //       lpCodeId,
  //       {
  //         decimals: 6,
  //         symbol: "USDT",
  //         name: "USDT token",
  //         initial_balances: [],
  //         mint: {
  //           minter: ics20Contract.contractAddress,
  //         },
  //       },
  //       "cw20-usdt"
  //     );
  //     usdtToken = new OraiswapTokenClient(
  //       oraiClient,
  //       oraiSenderAddress,
  //       usdtAddress
  //     );
  //     // deploy oracle addr
  //     const { contractAddress: oracleAddress } =
  //       await oraidexArtifacts.deployContract(
  //         oraiClient,
  //         oraiSenderAddress,
  //         {},
  //         "oraiswap-oracle",
  //         "oraiswap-oracle"
  //       );
  //     // deploy factory contract
  //     oracleContract = new OraiswapOracleClient(
  //       oraiClient,
  //       oraiSenderAddress,
  //       oracleAddress
  //     );

  //     await oracleContract.updateTaxRate({ rate: "0" });
  //     await oracleContract.updateTaxCap({ denom: AtomDenom, cap: "100000" });
  //     const { contractAddress: factoryAddress } =
  //       await oraidexArtifacts.deployContract(
  //         oraiClient,
  //         oraiSenderAddress,
  //         {
  //           commission_rate: "0",
  //           oracle_addr: oracleAddress,
  //           pair_code_id: pairCodeId,
  //           token_code_id: lpCodeId,
  //         },
  //         "oraiswap-factory",
  //         "oraiswap-factory"
  //       );

  //     const { contractAddress: routerAddress } =
  //       await oraidexArtifacts.deployContract(
  //         oraiClient,
  //         oraiSenderAddress,
  //         {
  //           factory_addr: factoryAddress,
  //           factory_addr_v2: factoryAddress,
  //         },
  //         "oraiswap-router",
  //         "oraiswap-router"
  //       );
  //     factoryContract = new OraiswapFactoryClient(
  //       oraiClient,
  //       oraiSenderAddress,
  //       factoryAddress
  //     );
  //     routerContract = new OraiswapRouterClient(
  //       oraiClient,
  //       oraiSenderAddress,
  //       routerAddress
  //     );

  //     // set correct router contract to prepare for the tests
  //     await ics20Contract.updateConfig({ swapRouterContract: routerAddress });
  //     // create mapping
  //     await ics20Contract.updateMappingPair({
  //       localAssetInfo: {
  //         token: {
  //           contract_addr: airiToken.contractAddress,
  //         },
  //       },
  //       localAssetInfoDecimals: 6,
  //       denom: airiIbcDenom,
  //       remoteDecimals: 6,
  //       localChannelId: channel,
  //       isMintBurn: true,
  //     });
  //     await ics20Contract.updateMappingPair({
  //       localAssetInfo: {
  //         token: {
  //           contract_addr: usdtToken.contractAddress,
  //         },
  //       },
  //       localAssetInfoDecimals: 6,
  //       denom: usdtIbcDenom,
  //       remoteDecimals: 6,
  //       localChannelId: channel,
  //       isMintBurn: true,
  //     });
  //     await factoryContract.createPair({
  //       assetInfos,
  //     });
  //     await factoryContract.createPair({
  //       assetInfos: [
  //         assetInfos[0],
  //         { token: { contract_addr: usdtToken.contractAddress } },
  //       ],
  //     });
  //     await factoryContract.createPair({
  //       assetInfos: [
  //         assetInfos[0],
  //         {
  //           native_token: {
  //             denom: AtomDenom,
  //           },
  //         },
  //       ],
  //     });

  //     airiOraiPair = await factoryContract.pair({
  //       assetInfos,
  //     });
  //     const secondPairInfo = await factoryContract.pair({
  //       assetInfos: [
  //         assetInfos[0],
  //         { token: { contract_addr: usdtToken.contractAddress } },
  //       ],
  //     });
  //     const thirdPairInfo = await factoryContract.pair({
  //       assetInfos: [
  //         assetInfos[0],
  //         {
  //           native_token: {
  //             denom: AtomDenom,
  //           },
  //         },
  //       ],
  //     });

  //     // mint lots of orai, airi for the pair contracts to mock provide lp
  //     // here, ratio is 1:1 => 1 AIRI = 1 ORAI
  //     oraiClient.app.bank.setBalance(
  //       airiOraiPair.contract_addr,
  //       coins(initialBalanceAmount, ORAI)
  //     );

  //     airiToken.sender = ics20Contract.contractAddress;
  //     await airiToken.mint({
  //       amount: initialBalanceAmount,
  //       recipient: airiOraiPair.contract_addr,
  //     });
  //     oraiClient.app.bank.setBalance(
  //       secondPairInfo.contract_addr,
  //       coins(initialBalanceAmount, ORAI)
  //     );

  //     usdtToken.sender = ics20Contract.contractAddress;
  //     await usdtToken.mint({
  //       amount: initialBalanceAmount,
  //       recipient: secondPairInfo.contract_addr,
  //     });
  //     oraiClient.app.bank.setBalance(thirdPairInfo.contract_addr, [
  //       coin(initialBalanceAmount, ORAI),
  //       coin(initialBalanceAmount, AtomDenom),
  //     ]);
  //   });

  //   it("mint-burn-test-simulate-withdraw-liquidity", async () => {
  //     // deploy another cw20 for oraiswap testing
  //     let scatomToken: OraiswapTokenClient;
  //     const atomIbc =
  //       "ibc/A2E2EEC9057A4A1C2C0A6A4C78B0239118DF5F278830F50B4A6BDD7A66506B78";
  //     const { contractAddress: scatomAddress } = await oraiClient.instantiate(
  //       oraiSenderAddress,
  //       lpId,
  //       {
  //         decimals: 6,
  //         symbol: "scATOM",
  //         name: "scATOM token",
  //         initial_balances: [
  //           { address: oraiSenderAddress, amount: initialBalanceAmount },
  //         ],
  //         mint: {
  //           minter: oraiSenderAddress,
  //         },
  //       },
  //       "cw20-scatom"
  //     );
  //     scatomToken = new OraiswapTokenClient(
  //       oraiClient,
  //       oraiSenderAddress,
  //       scatomAddress
  //     );
  //     const assetInfos = [
  //       { native_token: { denom: atomIbc } },
  //       { token: { contract_addr: scatomAddress } },
  //     ];
  //     await factoryContract.createPair({
  //       assetInfos,
  //     });
  //     const firstPairInfo = await factoryContract.pair({
  //       assetInfos,
  //     });
  //     const pairAddress = firstPairInfo.contract_addr;
  //     await scatomToken.increaseAllowance({
  //       amount: initialBalanceAmount,
  //       spender: pairAddress,
  //     });
  //     oraiClient.app.bank.setBalance(
  //       pairAddress,
  //       coins(initialBalanceAmount, atomIbc)
  //     );
  //     oraiClient.app.bank.setBalance(
  //       oraiSenderAddress,
  //       coins(initialBalanceAmount, atomIbc)
  //     );

  //     const pairContract = new OraiswapPairClient(
  //       oraiClient,
  //       oraiSenderAddress,
  //       pairAddress
  //     );
  //     await pairContract.provideLiquidity(
  //       {
  //         assets: [
  //           {
  //             amount: "10000000",
  //             info: { token: { contract_addr: scatomAddress } },
  //           },
  //           { amount: "10000000", info: { native_token: { denom: atomIbc } } },
  //         ],
  //       },
  //       "auto",
  //       undefined,
  //       [{ denom: atomIbc, amount: "10000000" }]
  //     );
  //     // query liquidity balance
  //     const lpToken = new OraiswapTokenClient(
  //       oraiClient,
  //       oraiSenderAddress,
  //       firstPairInfo.liquidity_token
  //     );
  //     const result = await lpToken.balance({ address: oraiSenderAddress });

  //     // set tax rate
  //     await oracleContract.updateTaxRate({ rate: "0.003" });
  //     await oracleContract.updateTaxCap({ denom: atomIbc, cap: "1000000" });

  //     // now we withdraw lp
  //     await lpToken.send({
  //       amount: "1000",
  //       contract: pairAddress,
  //       msg: Buffer.from(JSON.stringify({ withdraw_liquidity: {} })).toString(
  //         "base64"
  //       ),
  //     });
  //   });

  //   it("cw-ics20-test-single-step-cw20-token-swap-operations-to-dest-denom-FAILED-cannot-simulate-swap", async () => {
  //     // now send ibc package

  //     const unknownTokenOut = "foo";
  //     const swapAmount = toAmount(2000).toString();
  //     const minimumReceive = toAmount(1000).toString();
  //     // => dest token on Orai = ibc/EB7094899ACFB7A6F2A67DB084DEE2E9A83DEFAA5DEF92D9A9814FFD9FF673FA
  //     icsPackage.memo = buildUniversalSwapMemo(
  //       { minimumReceive, recoveryAddr: bobAddress },
  //       {
  //         swapAmount,
  //         returnAmount: swapAmount,
  //         routes: [
  //           {
  //             swapAmount,
  //             returnAmount: swapAmount,
  //             paths: [
  //               {
  //                 chainId: COSMOS_CHAIN_IDS.ORAICHAIN,
  //                 tokenIn: airiToken.contractAddress,
  //                 tokenOut: unknownTokenOut,
  //                 tokenOutChainId: COSMOS_CHAIN_IDS.ORAICHAIN,
  //                 tokenInAmount: swapAmount,
  //                 tokenOutAmount: swapAmount,
  //                 actions: [
  //                   {
  //                     type: "Swap",
  //                     tokenIn: airiToken.contractAddress,
  //                     tokenInAmount: swapAmount,
  //                     tokenOut: unknownTokenOut,
  //                     tokenOutAmount: swapAmount,
  //                     swapInfo: [
  //                       {
  //                         poolId: airiOraiPair.contract_addr,
  //                         tokenOut: unknownTokenOut,
  //                       },
  //                     ],
  //                   },
  //                 ],
  //               },
  //             ],
  //           },
  //         ],
  //       }
  //     );
  //     // transfer from cosmos to oraichain, should pass
  //     const result = await cosmosChain.ibc.sendPacketReceive({
  //       packet: {
  //         data: toBinary(icsPackage),
  //         ...packetData,
  //       },
  //       relayer: relayerAddress,
  //     });
  //     const wasmEvents = result.events.filter((ev) => ev.type === "wasm");
  //     console.dir(result, { depth: null });
  //     expect(
  //       wasmEvents.some((ev) =>
  //         ev.attributes.find(
  //           (attr) =>
  //             attr.key === "error_trying_to_call_entrypoint_for_universal_swap"
  //         )
  //       )
  //     ).toEqual(true);
  //     expect(
  //       wasmEvents.some((ev) =>
  //         ev.attributes.find((attr) =>
  //           attr.value.includes(
  //             "Error parsing into type oraiswap::asset::PairInfo"
  //           )
  //         )
  //       )
  //     ).toEqual(true);
  //   });

  //   it("mint-burn-cw-ics20-test-single-step-cw20-FAILED-IBC_TRANSFER_NATIVE_ERROR_ID-ack-SUCCESS", async () => {
  //     // fixture
  //     // icsPackage.memo = `unknown-channel/${bobAddress}:${usdtToken.contractAddress}`;

  //     // dest denom on orai: ibc/79E5EC9A42F2FC01B2BA609F13C985393779BE5153E01D24E79C2681B0DFB592
  //     icsPackage.memo = parseToIbcWasmMemo(bobAddress, "channel-15", "uatom");
  //     icsPackage.amount = initialBalanceAmount;

  //     // transfer from cosmos to oraichain, should pass
  //     const result = await cosmosChain.ibc.sendPacketReceive({
  //       packet: {
  //         data: toBinary(icsPackage),
  //         ...packetData,
  //       },
  //       relayer: relayerAddress,
  //     });
  //     console.log(result);
  //     // refunding also fails because of not enough balance to refund
  //     expect(
  //       findWasmEvent(result.events, "action", "ibc_transfer_native_error_id")
  //     ).not.toBeUndefined();
  //     // ack should be successful
  //     expect(result.acknowledgement).toEqual(
  //       Buffer.from('{"result":"MQ=="}').toString("base64")
  //     );
  //     expect(
  //       findWasmEvent(result.events, "undo_increase_channel", channel)
  //     ).toBeUndefined();

  //     // other types of reply id must not be called
  //     expect(
  //       findWasmEvent(result.events, "action", "swap_ops_failure_id")
  //     ).toBeUndefined();
  //     expect(
  //       findWasmEvent(result.events, "action", "native_receive_id")
  //     ).toBeUndefined();
  //     expect(
  //       findWasmEvent(result.events, "action", "follow_up_failure_id")
  //     ).toBeUndefined();
  //     expect(
  //       findWasmEvent(result.events, "action", "refund_failure_id")
  //     ).toBeUndefined();

  //     // for ibc native transfer case, we wont have refund either
  //     expect(
  //       result.events.find(
  //         (ev) =>
  //           ev.type === "wasm" &&
  //           ev.attributes.find(
  //             (attr) => attr.key === "action" && attr.value === "transfer"
  //           ) &&
  //           ev.attributes.find(
  //             (attr) => attr.key === "to" && attr.value === bobAddress
  //           )
  //       )
  //     ).toBeUndefined();
  //   });

  //   it("mint-burn-cw-ics20-test-single-step-cw20-success-FOLLOW_UP_IBC_SEND_FAILURE_ID-must-not-have-SWAP_OPS_FAILURE_ID-or-on_packet_failure-ack-SUCCESS", async () => {
  //     // fixture
  //     // icsPackage.memo = `${channel}/${bobAddress}:${airiToken.contractAddress}`;
  //     icsPackage.memo = parseToIbcWasmMemo(bobAddress, channel, airiIbcDenom);
  //     icsPackage.amount = initialBalanceAmount;
  //     // transfer from cosmos to oraichain, should pass
  //     const result = await cosmosChain.ibc.sendPacketReceive({
  //       packet: {
  //         data: toBinary(icsPackage),
  //         ...packetData,
  //       },
  //       relayer: relayerAddress,
  //     });
  //     // all id types of reply id must not be called, especially swap_ops_failure_id
  //     expect(
  //       findWasmEvent(result.events, "action", "swap_ops_failure_id")
  //     ).toBeUndefined();
  //     expect(
  //       findWasmEvent(result.events, "action", "native_receive_id")
  //     ).toBeUndefined();
  //     expect(
  //       findWasmEvent(result.events, "action", "follow_up_failure_id")
  //     ).toBeUndefined();
  //     expect(
  //       findWasmEvent(result.events, "action", "refund_failure_id")
  //     ).toBeUndefined();
  //     expect(
  //       findWasmEvent(result.events, "action", "ibc_transfer_native_error_id")
  //     ).toBeUndefined();
  //     expect(
  //       findWasmEvent(result.events, "action", "acknowledge")
  //     ).toBeUndefined();
  //     expect(
  //       findWasmEvent(result.events, "undo_reduce_channel", channel)
  //     ).toBeUndefined();
  //     // ack should be successful
  //     expect(result.acknowledgement).toEqual(
  //       Buffer.from('{"result":"MQ=="}').toString("base64")
  //     );

  //     // for ibc native transfer case, we wont have refund either
  //     expect(
  //       result.events.find(
  //         (ev) =>
  //           ev.type === "wasm" &&
  //           ev.attributes.find(
  //             (attr) => attr.key === "action" && attr.value === "transfer"
  //           ) &&
  //           ev.attributes.find(
  //             (attr) => attr.key === "to" && attr.value === bobAddress
  //           )
  //       )
  //     ).toBeUndefined();
  //   });

  //   it.each([
  //     [channel, "abcd", usdtIbcDenom], // hard-coded usdt address
  //     [channel, "0x", airiIbcDenom],
  //     [channel, "0xabcd", usdtIbcDenom],
  //     [channel, "tron-testnet0xabcd", airiIbcDenom], // bad evm address case
  //   ])(
  //     "mint-burn-cw-ics20-test-single-step-has-ibc-msg-dest-fail memo %s dest denom %s expected error",
  //     async (destChannel: string, destReceiver: string, destDenom: string) => {
  //       // now send ibc package
  //       // icsPackage.memo = `${destChannel}/${destReceiver}:${destDenom}`;
  //       icsPackage.memo = parseToIbcWasmMemo(
  //         destReceiver,
  //         destChannel,
  //         destDenom
  //       );
  //       // transfer from cosmos to oraichain, should pass
  //       const result = await cosmosChain.ibc.sendPacketReceive({
  //         packet: {
  //           data: toBinary(icsPackage),
  //           ...packetData,
  //         },
  //         relayer: relayerAddress,
  //       });
  //       const ibcEvent = result.events.find(
  //         (event) =>
  //           event.type === "transfer" &&
  //           event.attributes.find((attr) => attr.key === "channel")
  //       );
  //       // get swap operation event
  //       expect(ibcEvent).toBeUndefined();
  //       const ibcErrorMsg = result.attributes.find(
  //         (attr) =>
  //           attr.key === "ibc_error_msg" &&
  //           attr.value ===
  //             "Generic error: The destination info is neither evm or cosmos based"
  //       );
  //       expect(ibcErrorMsg).not.toBeUndefined();
  //     }
  //   );

  //   it.each([
  //     [channel, bridgeReceiver, airiIbcDenom], // hard-coded airi
  //   ])(
  //     "mint-burn-cw-ics20-test-single-step-has-ibc-msg-dest-receiver-evm-based memo %s dest denom %s expected recipient %s",
  //     async (destChannel: string, destReceiver: string, destDenom: string) => {
  //       // now send ibc package
  //       // icsPackage.memo = `${destChannel}/${destReceiver}:${destDenom}`;
  //       icsPackage.memo = parseToIbcWasmMemo(
  //         destReceiver,
  //         destChannel,
  //         destDenom
  //       );

  //       // transfer from cosmos to oraichain, should pass
  //       let result = await cosmosChain.ibc.sendPacketReceive({
  //         packet: {
  //           data: toBinary(icsPackage),
  //           ...packetData,
  //         },
  //         relayer: relayerAddress,
  //       });
  //       const sendPacketEvent = result.events.find(
  //         (event) => event.type === "send_packet"
  //       );
  //       expect(sendPacketEvent).not.toBeUndefined();
  //       const packetHex = sendPacketEvent.attributes.find(
  //         (attr) => attr.key === "packet_data_hex"
  //       ).value;
  //       expect(packetHex).not.toBeUndefined();
  //       const packet = JSON.parse(
  //         Buffer.from(packetHex, "hex").toString("ascii")
  //       );
  //       expect(packet.receiver).toEqual(icsPackage.sender);
  //       expect(packet.sender).toEqual(ics20Contract.contractAddress);
  //       // expect(packet.memo).toEqual(ics20Contract.contractAddress);

  //       // pass 1 day with 86_400 seconds
  //       cosmosChain.store.tx((setter) =>
  //         Ok(setter("time")(cosmosChain.time + 86_400 * 1e9))
  //       );

  //       // transfer from cosmos to oraichain, should pass
  //       result = await cosmosChain.ibc.sendPacketReceive({
  //         packet: {
  //           data: toBinary(icsPackage),
  //           ...packetData,
  //         },
  //         relayer: relayerAddress,
  //       });
  //       // expect(
  //       //   flatten(result.events.map((e) => e.attributes)).find((a) => a.key === 'error_follow_up_msgs').value
  //       // ).toContain('Generic error: timeout at');
  //     }
  //   );

  //   it("mint-burn-cw-ics20-test-single-step-ibc-msg-map-with-fee-denom-orai-and-airi-destination-denom-should-swap-normally", async () => {
  //     await ics20Contract.updateMappingPair({
  //       localAssetInfo: {
  //         native_token: {
  //           denom: ORAI,
  //         },
  //       },
  //       localAssetInfoDecimals: 6,
  //       denom: oraiIbcDenom,
  //       remoteDecimals: 6,
  //       localChannelId: channel,
  //     });

  //     let packetData = {
  //       src: {
  //         port_id: cosmosPort,
  //         channel_id: channel,
  //       },
  //       dest: {
  //         port_id: oraiPort,
  //         channel_id: channel,
  //       },
  //       sequence: 27,
  //       timeout: {
  //         block: {
  //           revision: 1,
  //           height: 12345678,
  //         },
  //       },
  //     };
  //     const icsPackage: FungibleTokenPacketData = {
  //       amount: ibcTransferAmount,
  //       denom: oraiIbcDenom,
  //       receiver: bobAddress,
  //       sender: cosmosSenderAddress,
  //       // memo: `${bobAddress}:${airiToken.contractAddress}`,
  //       memo: parseToIbcWasmMemo(bobAddress, "", airiToken.contractAddress),
  //     };
  //     // transfer from cosmos to oraichain, should pass
  //     let result = await cosmosChain.ibc.sendPacketReceive({
  //       packet: {
  //         data: toBinary(icsPackage),
  //         ...packetData,
  //       },
  //       relayer: relayerAddress,
  //     });

  //     const swapEvent = result.events.find(
  //       (event) =>
  //         event.type === "wasm" &&
  //         event.attributes.find((attr) => attr.value === "swap")
  //     );
  //     expect(
  //       swapEvent.attributes.filter(
  //         (attr) => attr.key === "offer_asset" && attr.value === ORAI
  //       ).length
  //     ).toBeGreaterThan(0);
  //     expect(
  //       swapEvent.attributes.filter(
  //         (attr) =>
  //           attr.key === "ask_asset" && attr.value === airiToken.contractAddress
  //       ).length
  //     ).toBeGreaterThan(0);
  //   });

  //   it("mint-burn-cw-ics20-test-single-step-ibc-msg-map-with-fee-denom-orai-and-orai-destination-denom-should-transfer-normally", async () => {
  //     await ics20Contract.updateMappingPair({
  //       localAssetInfo: {
  //         native_token: {
  //           denom: ORAI,
  //         },
  //       },
  //       localAssetInfoDecimals: 6,
  //       denom: oraiIbcDenom,
  //       remoteDecimals: 6,
  //       localChannelId: channel,
  //     });

  //     let packetData = {
  //       src: {
  //         port_id: cosmosPort,
  //         channel_id: channel,
  //       },
  //       dest: {
  //         port_id: oraiPort,
  //         channel_id: channel,
  //       },
  //       sequence: 27,
  //       timeout: {
  //         block: {
  //           revision: 1,
  //           height: 12345678,
  //         },
  //       },
  //     };
  //     const icsPackage: FungibleTokenPacketData = {
  //       amount: ibcTransferAmount,
  //       denom: oraiIbcDenom,
  //       receiver: bobAddress,
  //       sender: cosmosSenderAddress,
  //       // memo: `${bobAddress}:orai`,
  //       memo: parseToIbcWasmMemo(bobAddress, "", "orai"),
  //     };
  //     // transfer from cosmos to oraichain, should pass
  //     let result = await cosmosChain.ibc.sendPacketReceive({
  //       packet: {
  //         data: toBinary(icsPackage),
  //         ...packetData,
  //       },
  //       relayer: relayerAddress,
  //     });
  //     const transferEvent = result.events.find(
  //       (event) => event.type === "transfer"
  //     );
  //     expect(
  //       transferEvent.attributes.filter(
  //         (attr) => attr.key === "recipient" && attr.value === bobAddress
  //       ).length
  //     ).toBeGreaterThan(0);
  //     expect(
  //       transferEvent.attributes.filter(
  //         (attr) =>
  //           attr.key === "amount" &&
  //           attr.value ===
  //             JSON.stringify([{ denom: ORAI, amount: ibcTransferAmount }])
  //       ).length
  //     ).toBeGreaterThan(0);
  //   });

  //   describe("test-single-step-cosmos-based-ibc-transfer-native", () => {
  //     // unknowChannel is channel to cosmos
  //     const unknownChannel = "channel-15";
  //     beforeEach(async () => {
  //       // fixture
  //       // needs to fake a new ibc channel so that we can successfully do ibc transfer
  //       oraiClient.app.ibc.relay(
  //         unknownChannel,
  //         oraiPort,
  //         unknownChannel,
  //         cosmosPort,
  //         cosmosChain
  //       );
  //       await cosmosChain.ibc.sendChannelOpen({
  //         open_init: {
  //           channel: {
  //             counterparty_endpoint: {
  //               port_id: oraiPort,
  //               channel_id: unknownChannel,
  //             },
  //             endpoint: {
  //               port_id: cosmosPort,
  //               channel_id: unknownChannel,
  //             },
  //             order: IbcOrder.Unordered,
  //             version: "ics20-1",
  //             connection_id: "connection-0",
  //           },
  //         },
  //       });

  //       await cosmosChain.ibc.sendChannelConnect({
  //         open_ack: {
  //           channel: {
  //             counterparty_endpoint: {
  //               port_id: oraiPort,
  //               channel_id: unknownChannel,
  //             },
  //             endpoint: {
  //               port_id: cosmosPort,
  //               channel_id: unknownChannel,
  //             },
  //             order: IbcOrder.Unordered,
  //             version: "ics20-1",
  //             connection_id: "connection-0",
  //           },
  //           counterparty_version: "ics20-1",
  //         },
  //       });

  //       cosmosChain.ibc.addMiddleWare((msg, app) => {
  //         if ("packet" in msg.data) {
  //           const data = msg.data.packet as IbcPacket;
  //           if (Number(data.timeout.timestamp) < cosmosChain.time) {
  //             throw new GenericError("timeout at " + data.timeout.timestamp);
  //           }
  //         }
  //       });
  //     });

  //     it.each([
  //       ["channel-15", "orai1g4h64yjt0fvzv5v2j8tyfnpe5kmnetejvfgs7g", "uatom"], // edge case, dest denom is also airi
  //     ])(
  //       "mint-burn-cw-ics20-test-single-step-has-ibc-msg-dest-receiver-cosmos-based dest channel %s dest denom %s expected recipient %s",
  //       async (
  //         destChannel: string,
  //         destReceiver: string,
  //         destDenom: string
  //       ) => {
  //         // now send ibc package
  //         // icsPackage.memo = `${destChannel}/${destReceiver}:${destDenom}`;
  //         icsPackage.memo = parseToIbcWasmMemo(
  //           destReceiver,
  //           destChannel,
  //           destDenom
  //         );
  //         // transfer from cosmos to oraichain, should pass
  //         const result = await cosmosChain.ibc.sendPacketReceive({
  //           packet: {
  //             data: toBinary(icsPackage),
  //             ...packetData,
  //           },
  //           relayer: relayerAddress,
  //         });

  //         const ibcEvent = result.events.find(
  //           (event) =>
  //             event.type === "transfer" &&
  //             event.attributes.find((attr) => attr.key === "channel")
  //         );

  //         // get swap operation event
  //         expect(ibcEvent).not.toBeUndefined();
  //         expect(
  //           ibcEvent.attributes.find((attr) => attr.key === "channel").value
  //         ).toEqual(destChannel);
  //         expect(
  //           ibcEvent.attributes.find((attr) => attr.key === "recipient").value
  //         ).toEqual(destReceiver);
  //         expect(
  //           ibcEvent.attributes.find((attr) => attr.key === "sender").value
  //         ).toEqual(ics20Contract.contractAddress);
  //         expect(
  //           ibcEvent.attributes.find((attr) => attr.key === "amount").value
  //         ).toContain(AtomDenom);
  //       }
  //     );

  //     it("mint-burn-cw-ics20-test-single-step-ibc-msg-SUCCESS-map-with-fee-denom-orai-and-orai-destination-denom-with-dest-channel-not-matched-with-mapping-pair-should-do-ibctransfer", async () => {
  //       await ics20Contract.updateMappingPair({
  //         localAssetInfo: {
  //           native_token: {
  //             denom: ORAI,
  //           },
  //         },
  //         localAssetInfoDecimals: 6,
  //         denom: oraiIbcDenom,
  //         remoteDecimals: 6,
  //         localChannelId: channel,
  //       });

  //       let packetData = {
  //         src: {
  //           port_id: cosmosPort,
  //           channel_id: channel,
  //         },
  //         dest: {
  //           port_id: oraiPort,
  //           channel_id: channel,
  //         },
  //         sequence: 27,
  //         timeout: {
  //           block: {
  //             revision: 1,
  //             height: 12345678,
  //           },
  //         },
  //       };
  //       const icsPackage: FungibleTokenPacketData = {
  //         amount: ibcTransferAmount,
  //         denom: oraiIbcDenom,
  //         receiver: bobAddress,
  //         sender: cosmosSenderAddress,
  //         // memo: `${unknownChannel}/${bobAddress}:orai`,
  //         memo: parseToIbcWasmMemo(bobAddress, atomChannel, "uatom"),
  //       };
  //       // transfer from cosmos to oraichain, should pass
  //       let result = await cosmosChain.ibc.sendPacketReceive({
  //         packet: {
  //           data: toBinary(icsPackage),
  //           ...packetData,
  //         },
  //         relayer: relayerAddress,
  //       });

  //       const transferEvent = result.events.find(
  //         (event) =>
  //           event.type === "transfer" &&
  //           event.attributes.find((attr) => attr.key === "channel")
  //       );
  //       console.log(transferEvent);
  //       expect(
  //         transferEvent.attributes.filter(
  //           (attr) => attr.key === "recipient" && attr.value === bobAddress
  //         ).length
  //       ).toBeGreaterThan(0);
  //       expect(
  //         transferEvent.attributes.filter(
  //           (attr) => attr.key === "amount" && attr.value.includes(AtomDenom)
  //         ).length
  //       ).toBeGreaterThan(0);
  //       expect(
  //         transferEvent.attributes.filter(
  //           (attr) => attr.key === "channel" && attr.value === unknownChannel
  //         ).length
  //       ).toBeGreaterThan(0);
  //     });
  //   });

  //   it("mint-burn-cw-ics20-test-single-step-handle_ibc_packet_receive_native_remote_chain-has-relayer-fee-should-be-deducted", async () => {
  //     // setup relayer fee
  //     const relayerFee = "100000";
  //     await ics20Contract.updateConfig({
  //       relayerFee: [{ prefix: "tron-testnet", fee: relayerFee }],
  //     });

  //     const icsPackage: FungibleTokenPacketData = {
  //       amount: ibcTransferAmount,
  //       denom: airiIbcDenom,
  //       receiver: bobAddress,
  //       sender: oraibridgeSenderAddress,
  //       memo: parseToIbcWasmMemo(bobAddress, channel, oraiIbcDenom),
  //     };
  //     // transfer from cosmos to oraichain, should pass
  //     let result = await cosmosChain.ibc.sendPacketReceive({
  //       packet: {
  //         data: toBinary(icsPackage),
  //         ...packetData,
  //       },
  //       relayer: relayerAddress,
  //     });
  //     const hasRelayerFee = result.events.find(
  //       (event) =>
  //         event.type === "wasm" &&
  //         event.attributes.find(
  //           (attr) => attr.key === "to" && attr.value === relayerAddress
  //         ) &&
  //         event.attributes.find(
  //           (attr) => attr.key === "amount" && attr.value === relayerFee
  //         )
  //     );
  //     expect(hasRelayerFee).not.toBeUndefined();
  //     expect(
  //       result.attributes.find(
  //         (attr) => attr.key === "relayer_fee" && attr.value === relayerFee
  //       )
  //     ).not.toBeUndefined();
  //   });

  //   it.each<[string, string]>([
  //     [parseToIbcWasmMemo(bobAddress, channel, oraiIbcDenom), "20000000"],
  //     [parseToIbcWasmMemo(bobAddress, "", "orai"), "10000000"],
  //   ])(
  //     "mint-burn-cw-ics20-test-single-step-ibc-handle_ibc_packet_receive_native_remote_chain-has-token-fee-should-be-deducted",
  //     async (memo, expectedTokenFee) => {
  //       // setup relayer fee
  //       await ics20Contract.updateConfig({
  //         tokenFee: [
  //           {
  //             token_denom: airiIbcDenom,
  //             ratio: { nominator: 1, denominator: 10 },
  //           },
  //         ],
  //       });

  //       const icsPackage: FungibleTokenPacketData = {
  //         amount: ibcTransferAmount,
  //         denom: airiIbcDenom,
  //         receiver: bobAddress,
  //         sender: oraibridgeSenderAddress,
  //         memo,
  //       };
  //       // transfer from cosmos to oraichain, should pass
  //       let result = await cosmosChain.ibc.sendPacketReceive({
  //         packet: {
  //           data: toBinary(icsPackage),
  //           ...packetData,
  //         },
  //         relayer: relayerAddress,
  //       });
  //       const hasTokenFee = result.events.filter(
  //         (event) =>
  //           event.type === "wasm" &&
  //           event.attributes.find(
  //             (attr) => attr.key === "to" && attr.value === senderAddress
  //           )
  //       );
  //       expect(hasTokenFee).not.toBeUndefined();
  //       expect(
  //         result.attributes.find(
  //           (attr) => attr.key === "token_fee" && expectedTokenFee
  //         )
  //       ).not.toBeUndefined();
  //     }
  //   );

  //   it.each<[string, string, string]>([
  //     [
  //       parseToIbcWasmMemo(bobAddress, channel, airiIbcDenom),
  //       "20000000",
  //       "100000",
  //     ],
  //     [
  //       parseToIbcWasmMemo(bridgeReceiver, channel, airiIbcDenom),
  //       "20000000",
  //       "200000",
  //     ], // double deducted when there's an outgoing ibc msg after receiving the packet
  //     [parseToIbcWasmMemo(bobAddress, "", "orai"), "10000000", "100000"],
  //   ])(
  //     "mint-burn-test-handle_ibc_packet_receive_native_remote_chain-has-both-token-fee-and-relayer-fee-should-be-both-deducted-given memo %s should give expected token fee %s and expected relayer fee %s",
  //     async (memo, expectedTokenFee, expectedRelayerFee) => {
  //       // setup relayer fee
  //       const relayerFee = "100000";
  //       await ics20Contract.updateConfig({
  //         tokenFee: [
  //           {
  //             token_denom: airiIbcDenom,
  //             ratio: { nominator: 1, denominator: 10 },
  //           },
  //           { token_denom: "orai", ratio: { nominator: 1, denominator: 10 } },
  //         ],
  //         relayerFee: [{ prefix: "tron-testnet", fee: relayerFee }],
  //       });

  //       const icsPackage: FungibleTokenPacketData = {
  //         amount: ibcTransferAmount,
  //         denom: airiIbcDenom,
  //         receiver: bobAddress,
  //         sender: oraibridgeSenderAddress,
  //         memo,
  //       };
  //       // transfer from cosmos to oraichain, should pass
  //       let result = await cosmosChain.ibc.sendPacketReceive({
  //         packet: {
  //           data: toBinary(icsPackage),
  //           ...packetData,
  //         },
  //         relayer: relayerAddress,
  //       });

  //       const hasRelayerFee = result.events.find(
  //         (event) =>
  //           event.type === "wasm" &&
  //           event.attributes.find(
  //             (attr) => attr.key === "to" && attr.value === relayerAddress
  //           ) &&
  //           event.attributes.find(
  //             (attr) =>
  //               attr.key === "amount" && attr.value === expectedRelayerFee
  //           )
  //       );
  //       expect(hasRelayerFee).not.toBeUndefined();
  //       expect(
  //         result.attributes.find(
  //           (attr) =>
  //             attr.key === "relayer_fee" && attr.value === expectedRelayerFee
  //         )
  //       ).not.toBeUndefined();

  //       const hasTokenFee = result.events.find(
  //         (event) =>
  //           event.type === "wasm" &&
  //           event.attributes.find(
  //             (attr) => attr.key === "to" && attr.value === senderAddress
  //           ) &&
  //           event.attributes.find(
  //             (attr) => attr.key === "amount" && attr.value === expectedTokenFee
  //           )
  //       );
  //       expect(hasTokenFee).not.toBeUndefined();
  //       expect(
  //         result.attributes.find(
  //           (attr) =>
  //             attr.key === "token_fee" && attr.value === expectedTokenFee
  //         )
  //       ).not.toBeUndefined();
  //     }
  //   );

  //   it.each<[string, string, string, string, string]>([
  //     [
  //       ibcTransferAmount,
  //       ibcTransferAmount,
  //       "10000000",
  //       "90000000",
  //       ibcTransferAmount,
  //     ],
  //   ])(
  //     "mint-burn-cw-ics20-test-single-step-handle_ibc_packet_receive_native_remote_chain-deducted-amount-is-zero-should-still-charge-fees",
  //     async (
  //       transferAmount,
  //       relayerFee,
  //       expectedTokenFee,
  //       expectedRelayerFee,
  //       expectedTotalFee
  //     ) => {
  //       await ics20Contract.updateConfig({
  //         tokenFee: [
  //           {
  //             token_denom: airiIbcDenom,
  //             ratio: { nominator: 1, denominator: 10 },
  //           },
  //         ],
  //         relayerFee: [{ prefix: "tron-testnet", fee: relayerFee }],
  //       });

  //       const icsPackage: FungibleTokenPacketData = {
  //         amount: transferAmount,
  //         denom: airiIbcDenom,
  //         receiver: bobAddress,
  //         sender: oraibridgeSenderAddress,
  //         memo: parseToIbcWasmMemo(bobAddress, "", "orai"),
  //       };
  //       // transfer from cosmos to oraichain, should pass
  //       let result = await cosmosChain.ibc.sendPacketReceive({
  //         packet: {
  //           data: toBinary(icsPackage),
  //           ...packetData,
  //         },
  //         relayer: relayerAddress,
  //       });

  //       const hasFees = result.events.find(
  //         (event) =>
  //           event.type === "wasm" &&
  //           event.attributes.find(
  //             (attr) => attr.key === "to" && attr.value === senderAddress
  //           ) &&
  //           event.attributes.find(
  //             (attr) => attr.key === "amount" && attr.value === expectedTotalFee
  //           )
  //       );

  //       expect(hasFees).not.toBeUndefined();
  //       expect(
  //         result.attributes.find(
  //           (attr) =>
  //             attr.key === "token_fee" && attr.value === expectedTokenFee
  //         )
  //       ).not.toBeUndefined();
  //       expect(
  //         result.attributes.find(
  //           (attr) =>
  //             attr.key === "relayer_fee" && attr.value === expectedRelayerFee
  //         )
  //       ).not.toBeUndefined();
  //     }
  //   );

  //   // execute transfer to remote test cases
  //   it("mint-burn-test-execute_transfer_back_to_remote_chain-native-FAILED-no-funds-sent", async () => {
  //     oraiClient.app.bank.setBalance(
  //       senderAddress,
  //       coins(initialBalanceAmount, ORAI)
  //     );
  //     try {
  //       await ics20Contract.transferToRemote(
  //         {
  //           localChannelId: "1",
  //           memo: null,
  //           remoteAddress: "a",
  //           remoteDenom: "a",
  //           timeout: 60,
  //         },
  //         "auto",
  //         null
  //       );
  //     } catch (error) {
  //       expect(error.toString()).toContain("No funds sent");
  //     }
  //   });

  //   it("mint-burn-test-execute_transfer_back_to_remote_chain-native-FAILED-no-mapping-found", async () => {
  //     oraiClient.app.bank.setBalance(
  //       senderAddress,
  //       coins(initialBalanceAmount, ORAI)
  //     );
  //     try {
  //       await ics20Contract.transferToRemote(
  //         {
  //           localChannelId: "1",
  //           memo: null,
  //           remoteAddress: "a",
  //           remoteDenom: "a",
  //           timeout: 60,
  //         },
  //         "auto",
  //         null,
  //         [{ denom: ORAI, amount: "100" }]
  //       );
  //     } catch (error) {
  //       expect(error.toString()).toContain("Could not find the mapping pair");
  //     }
  //   });

  //   it("mint-burn-test-execute_transfer_back_to_remote_chain-native-FAILED-no-mapping-found", async () => {
  //     oraiClient.app.bank.setBalance(
  //       senderAddress,
  //       coins(initialBalanceAmount, ORAI)
  //     );
  //     try {
  //       await ics20Contract.transferToRemote(
  //         {
  //           localChannelId: "1",
  //           memo: null,
  //           remoteAddress: "a",
  //           remoteDenom: "a",
  //           timeout: 60,
  //         },
  //         "auto",
  //         null,
  //         [{ denom: ORAI, amount: "100" }]
  //       );
  //     } catch (error) {
  //       expect(error.toString()).toContain("Could not find the mapping pair");
  //     }
  //   });
  // });
});
