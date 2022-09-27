## Installation

```bash

# run the build container for oraid. Wait for it to finish
docker compose -f docker-compose.build.yml up
```

## Start the networks

```bash
docker-compose up -d
```

## deploy smart contract

```bash
# build smart contract
./scripts/build_contract.sh contracts/cw20-ics20
cp contracts/cw20-ics20/artifacts/cw20-ics20.wasm .mars

# build cw20
./scripts/build_contract.sh contracts/cw20-base
cp contracts/cw20-base/artifacts/cw20-base.wasm .mars

# go to mars network
docker compose exec mars ash

./scripts/deploy_contract.sh .mars/cw20-ics20.wasm 'cw20-ics20' '{"default_timeout":90}'

./scripts/deploy_contract.sh .mars/cw20-base.wasm 'cw20-base' '{"name":"EARTH","symbol":"EARTH","decimals":6,"initial_balances":[{"address":"mars10pyejy66429refv3g35g2t7am0was7ya90pn2w","amount":"100000000000000"}],"mint":{"minter":"mars15ez8l0c2qte2sa0a4xsdmaswy96vzj2fl2ephq"}}'

# mint token for cw20-ics20 (optional)
oraid tx wasm execute mars18vd8fpwxzck93qlwghaj6arh4p7c5n89plpqv0 '{"mint":{"recipient":"mars10pyejy66429refv3g35g2t7am0was7ya90pn2w","amount":"100000000000000000000000"}}' --keyring-backend test --from $USER --chain-id $CHAIN_ID -y


# migrate contract
./scripts/migrate_contract.sh .mars/cw20-ics20.wasm mars10pyejy66429refv3g35g2t7am0was7ya90pn2w # migrate to test changing cw20 contract
```

## start relayer

```bash

docker-compose exec hermes bash
hermes --config config.toml keys add --chain Earth --mnemonic-file accounts/Earth.txt
hermes --config config.toml keys add --chain Mars --mnemonic-file accounts/Mars.txt

# create a channel
hermes --config config.toml create channel --a-chain Earth --b-chain Mars --a-port transfer --b-port wasm.mars10pyejy66429refv3g35g2t7am0was7ya90pn2w --new-client-connection

# start hermes
hermes --config config.toml start
```

## send cross-channel

```bash
# from earth to mars on channel
docker compose exec earth ash
oraid tx ibc-transfer transfer transfer channel-0 mars15ez8l0c2qte2sa0a4xsdmaswy96vzj2fl2ephq 10000000earth --from duc --chain-id Earth -y --keyring-backend test
# check mars balance
docker compose exec mars ash
oraid query wasm contract-state smart mars18vd8fpwxzck93qlwghaj6arh4p7c5n89plpqv0 '{"balance":{"address":"mars15ez8l0c2qte2sa0a4xsdmaswy96vzj2fl2ephq"}}'

# from mars to earth send back
# send back command
oraid tx wasm execute mars18vd8fpwxzck93qlwghaj6arh4p7c5n89plpqv0 '{"send":{"amount":"10000000","contract":"mars10pyejy66429refv3g35g2t7am0was7ya90pn2w","msg":"'$(echo '{"channel":"channel-0","remote_address":"earth1w84gt7t7dzvj6qmf5q73d2yzyz35uwc7y8fkwp"}' | base64 -w 0)'"}}' --from $USER --chain-id $CHAIN_ID -y --keyring-backend test
```

# TODO:

remove hard code & update dynamic logic for cw20-ics20. Now the demo is for prototype only (proof of concept)