# The hunt, second language

## Reach

12 of 18 scope entries carry code this analyser reads, 170 of 2121 files in all. A further 1158 files sit outside the scope by the programmes' own exclusions.

| Tier | Target | Programme | Commit | Files in scope | Readable | Written in |
|---|---|---|---|---|---|---|
| core | Simplex, Validator Node, Full Node, DHT Node | TON security bug bounty | 3d478cbde854 | 1562 | none | cpp 492, h 489, py 129 |
| standard contracts | Network config, elector, simple and v3 wallets | TON security bug bounty | 3d478cbde854 | 4 | 4 | fc 4 |
| standard contracts | Wallet V4 and subscriptions | TON security bug bounty | 68b56dc0f7f6 | 7 | 2 | fc 2, no extension 2, fif 1 |
| standard contracts | Multisig V2 | TON security bug bounty | 9a4b13df6345 | 47 | 10 | ts 16, func 9, json 6 |
| standard contracts | Highload V3 | TON security bug bounty | f5d9b592bd5a | 23 | 2 | ts 8, no extension 5, json 4 |
| standard contracts | Nominator pool | TON security bug bounty | 2f35c36b5ad6 | 69 | 3 | js 47, fif 12, fc 3 |
| standard contracts | Fungible, non-fungible and semi-fungible tokens | TON security bug bounty | 1182ad994132 | 56 | 19 | fc 19, fif 16, ts 11 |
| standard contracts | TON DNS | TON security bug bounty | d08131031fb6 | 36 | 7 | js 20, fc 7, fif 4 |
| tools | MyTonCtrl validator tools | TON security bug bounty | 64c2aa946a4e | 137 | 1 | py 98, sh 17, fif 10 |
| tools | HTTP API | TON security bug bounty | 4b4e54fc2045 | 27 | none | py 10, sh 5, no extension 4 |
| tools | Python SDK | TON security bug bounty | 391861fec060 | 28 | none | py 12, no extension 3, so 3 |
| ecosystem | STON.fi DEX v1.0.0 | STON.fi Bug Bounty | b124d4e02d6d | 4 | 4 | func 4 |
| ecosystem | EVAA lending contracts | EVAA, HackenProof | no repository named | 0 | not resolvable | assets listed on a platform this run cannot read |
| ecosystem | Torch Stable Swap | Torch Finance Bug Bounty | bb9d1673864f | 44 | 44 | fc 44 |
| ecosystem | Telegram USD, tgUSD | Torch Finance Bug Bounty | e643ad49d13a | 58 | 57 | tolk 49, fc 8, tlb 1 |
| ecosystem | Ton Whales nominator pool | Ton Whales Bounty Program | 0553e1b6ddfc | 19 | 17 | fc 17, md 1, no extension 1 |
| ecosystem | Tonkeeper products and services | Tonkeeper Bug Bounty | no repository named | 0 | not resolvable | products and services, no repository named |
| frontend | TON web services | TON Society, HackenProof | no repository named | 0 | not resolvable | sites and frontend, reported through the platform |

## The run

170 files analysed, 226 findings, 0 files read only in part, 0 unreadable.

### Network config, elector, simple and v3 wallets — TON security bug bounty

At `3d478cbde854`, 4 files analysed.

| Detector | Severity | Where | What |
|---|---|---|---|
| LEP | Info | crypto/smartcont/config-code.fc:151 | function `unpack_validator_descr` reads `slice` parameter `cs` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | crypto/smartcont/config-code.fc:235 | function `update_proposal_status` reads `slice` parameter `rest` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | crypto/smartcont/config-code.fc:383 | function `register_voting_proposal` reads `slice` parameter `cs` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | crypto/smartcont/elector-code.fc:144 | function `unpack_validator_descr` reads `slice` parameter `cs` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |

### Wallet V4 and subscriptions — TON security bug bounty

At `68b56dc0f7f6`, 2 files analysed.

| Detector | Severity | Where | What |
|---|---|---|---|
| LEP | Info | func/wallet-v4-code.fc:8 | function `recv_internal` reads `slice` parameter `in_msg` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |

### Multisig V2 — TON security bug bounty

At `9a4b13df6345`, 10 files analysed.

| Detector | Severity | Where | What |
|---|---|---|---|
| LEP | Info | contracts/multisig.func:84 | function `recv_internal` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |

### Highload V3 — TON security bug bounty

At `f5d9b592bd5a`, 2 files analysed.

| Detector | Severity | Where | What |
|---|---|---|---|
| LEP | Info | contracts/highload-wallet-v3.func:29 | function `recv_internal` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |

### Nominator pool — TON security bug bounty

At `2f35c36b5ad6`, 3 files analysed.

| Detector | Severity | Where | What |
|---|---|---|---|
| LEP | Info | func/pool.fc:153 | function `unpack_nominator` reads `slice` parameter `ds` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | func/pool.fc:170 | function `unpack_config` reads `slice` parameter `ds` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |

### Fungible, non-fungible and semi-fungible tokens — TON security bug bounty

At `1182ad994132`, 19 files analysed.

| Detector | Severity | Where | What |
|---|---|---|---|
| LEP | Info | ft/jetton-discovery.fc:18 | function `recv_internal` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | ft/jetton-minter-ICO.fc:39 | function `recv_internal` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | ft/jetton-minter-discoverable.fc:43 | function `recv_internal` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | ft/jetton-minter.fc:42 | function `recv_internal` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | ft/jetton-wallet.fc:108 | function `receive_tokens` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | ft/jetton-wallet.fc:163 | function `burn_tokens` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | ft/jetton-wallet.fc:197 | function `on_bounce` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | nft/nft-collection-editable.fc:70 | function `recv_internal` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | nft/nft-collection.fc:70 | function `recv_internal` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | nft/nft-marketplace.fc:20 | function `recv_internal` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | nft/nft-sale.fc:84 | function `recv_internal` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |

### TON DNS — TON security bug bounty

At `d08131031fb6`, 7 files analysed.

| Detector | Severity | Where | What |
|---|---|---|---|
| LEP | Info | func/dns-utils.fc:22 | function `get_top_domain_bits` reads `slice` parameter `domain` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | func/dns-utils.fc:36 | function `read_domain_from_comment` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |

### MyTonCtrl validator tools — TON security bug bounty

At `64c2aa946a4e`, 1 files analysed.

Nothing fired. The search did not reach a defect within this budget under this scope.

### STON.fi DEX v1.0.0 — STON.fi Bug Bounty

At `b124d4e02d6d`, 4 files analysed.

| Detector | Severity | Where | What |
|---|---|---|---|
| LEP | Info | contracts/lp_wallet.func:51 | function `receive_tokens` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | contracts/lp_wallet.func:100 | function `burn_tokens` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | contracts/lp_wallet.func:131 | function `on_bounce` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |

### Torch Stable Swap — Torch Finance Bug Bounty

At `bb9d1673864f`, 44 files analysed.

| Detector | Severity | Where | What |
|---|---|---|---|
| LEP | Info | contracts/base_contract/main.fc:20 | function `route_internal` reads `slice` parameter `ctx::body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | contracts/jetton/jetton_wallet.fc:73 | function `receive_jettons` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | contracts/jetton/jetton_wallet.fc:130 | function `on_bounce` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | contracts/pools/utils.fc:63 | function `unpack_swap_next` reads `slice` parameter `next_slice` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | contracts/pools/utils.fc:79 | function `unpack_deposit_next` reads `slice` parameter `next_slice` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | contracts/pools/utils.fc:95 | function `unpack_withdraw_next` reads `slice` parameter `next_slice` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |

### Telegram USD, tgUSD — Torch Finance Bug Bounty

At `e643ad49d13a`, 57 files analysed.

| Detector | Severity | Where | What |
|---|---|---|---|
| DEPR | Info | contracts/common/log.tolk:4 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/log.tolk:5 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/log.tolk:6 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/log.tolk:7 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/log.tolk:12 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/log.tolk:21 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/log.tolk:29 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:4 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:8 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:9 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:10 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:12 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:13 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:14 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:17 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:18 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:19 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:20 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:21 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:22 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:23 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:24 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:25 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:26 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:27 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:28 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:29 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:30 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:31 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:32 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:33 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:34 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:35 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:38 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:39 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:40 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:41 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:42 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:43 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:44 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:45 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:46 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:47 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:48 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:49 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:52 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:55 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:56 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:57 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/op-codes.tolk:58 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/send.tolk:74 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/send.tolk:73 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/send.tolk:118 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/send.tolk:117 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/send.tolk:129 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/send.tolk:128 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:79 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:78 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:115 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:114 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:128 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:127 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:139 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:138 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:150 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:149 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:162 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:161 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:171 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:181 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/common/serdes.tolk:180 | `storeMessageOp` is deprecated in Tolk's own standard library |
| LEP | Info | contracts/jetton/jetton-wallet.fc:89 | function `receive_jettons` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | contracts/jetton/jetton-wallet.fc:185 | function `on_bounce` reads `slice` parameter `in_msg_body` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | contracts/redeem-account/main.tolk:28 | function `routeInternal` reads `slice` parameter `ctxSender` but does not call `.assertEnd()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | contracts/redeem-account/main.tolk:28 | function `routeInternal` reads `slice` parameter `ctxBody` but does not call `.assertEnd()` on every path — trailing data the caller attached would be silently ignored |
| AC | Medium | contracts/redeem-account/main.tolk:221 | function `onInternalMessage` calls `routeInternal()` with no recognized check first; `routeInternal` mutates storage on some path with no guard of its own — an unauthenticated caller may reach it |
| DEPR | Info | contracts/redeem-account/main.tolk:31 | `loadMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/redeem-account/main.tolk:106 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Low | contracts/redeem-account/main.tolk:190 | `setContractCodePostponed` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Low | contracts/redeem-account/main.tolk:194 | `getContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Low | contracts/redeem-account/main.tolk:196 | `setContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Info | contracts/redeem-account/main.tolk:209 | `loadMessageFlags` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/redeem-account/serdes.tolk:36 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/redeem-account/serdes.tolk:35 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Low | contracts/redeem-account/storage.tolk:14 | `getContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Low | contracts/redeem-account/storage.tolk:30 | `setContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Info | contracts/tgusd-engine/log.tolk:8 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:9 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:10 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:11 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:12 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:13 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:14 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:15 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:16 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:17 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:18 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:19 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:20 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:21 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:22 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:23 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:24 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:29 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:40 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:50 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:61 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:70 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:80 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:91 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:113 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:124 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:135 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:144 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:153 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:162 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/log.tolk:171 | `storeMessageOp` is deprecated in Tolk's own standard library |
| AC | Medium | contracts/tgusd-engine/main.tolk:914 | function `onInternalMessage` calls `routeInternal()` with no recognized check first; `routeInternal` mutates storage on some path with no guard of its own — an unauthenticated caller may reach it |
| DEPR | Info | contracts/tgusd-engine/main.tolk:40 | `loadMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/main.tolk:486 | `reserveToncoinsOnBalance` is deprecated in Tolk's own standard library |
| DEPR | Low | contracts/tgusd-engine/main.tolk:750 | `setContractCodePostponed` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Low | contracts/tgusd-engine/main.tolk:754 | `getContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Low | contracts/tgusd-engine/main.tolk:756 | `setContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Info | contracts/tgusd-engine/main.tolk:841 | `createEmptyTuple` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/main.tolk:902 | `loadMessageFlags` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/serdes.tolk:12 | `createEmptyTuple` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/serdes.tolk:217 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/serdes.tolk:216 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/serdes.tolk:226 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/serdes.tolk:225 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/serdes.tolk:234 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/serdes.tolk:247 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/serdes.tolk:246 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/serdes.tolk:256 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-engine/serdes.tolk:255 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Low | contracts/tgusd-engine/storage.tolk:19 | `getContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Low | contracts/tgusd-engine/storage.tolk:46 | `setContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| LEP | Info | contracts/tgusd-staking/guard.tolk:156 | function `requireJettonMasterAddressNotTaken` reads `slice` parameter `tgUSDJettonWallet` but does not call `.assertEnd()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | contracts/tgusd-staking/guard.tolk:156 | function `requireJettonMasterAddressNotTaken` reads `slice` parameter `stgUSDJettonWallet` but does not call `.assertEnd()` on every path — trailing data the caller attached would be silently ignored |
| DEPR | Info | contracts/tgusd-staking/log.tolk:4 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:5 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:6 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:7 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:8 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:9 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:10 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:11 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:12 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:13 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:14 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:15 | `stringCrc32` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:20 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:31 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:48 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:74 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:95 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:119 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:140 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:151 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:161 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:173 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/log.tolk:182 | `storeMessageOp` is deprecated in Tolk's own standard library |
| AC | Medium | contracts/tgusd-staking/main.tolk:695 | function `onInternalMessage` calls `routeInternal()` with no recognized check first; `routeInternal` mutates storage on some path with no guard of its own — an unauthenticated caller may reach it |
| DEPR | Info | contracts/tgusd-staking/main.tolk:36 | `loadMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/main.tolk:356 | `reserveToncoinsOnBalance` is deprecated in Tolk's own standard library |
| DEPR | Low | contracts/tgusd-staking/main.tolk:561 | `setContractCodePostponed` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Low | contracts/tgusd-staking/main.tolk:565 | `getContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Low | contracts/tgusd-staking/main.tolk:567 | `setContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Info | contracts/tgusd-staking/main.tolk:683 | `loadMessageFlags` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/serdes.tolk:95 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/serdes.tolk:94 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/serdes.tolk:105 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/serdes.tolk:104 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/serdes.tolk:118 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/tgusd-staking/serdes.tolk:117 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Low | contracts/tgusd-staking/storage.tolk:24 | `getContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Low | contracts/tgusd-staking/storage.tolk:60 | `setContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| LEP | Info | contracts/unstake-account/main.tolk:29 | function `routeInternal` reads `slice` parameter `ctxSender` but does not call `.assertEnd()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | contracts/unstake-account/main.tolk:29 | function `routeInternal` reads `slice` parameter `ctxBody` but does not call `.assertEnd()` on every path — trailing data the caller attached would be silently ignored |
| AC | Medium | contracts/unstake-account/main.tolk:166 | function `onInternalMessage` calls `routeInternal()` with no recognized check first; `routeInternal` mutates storage on some path with no guard of its own — an unauthenticated caller may reach it |
| DEPR | Info | contracts/unstake-account/main.tolk:32 | `loadMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Low | contracts/unstake-account/main.tolk:134 | `setContractCodePostponed` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Low | contracts/unstake-account/main.tolk:138 | `getContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Low | contracts/unstake-account/main.tolk:140 | `setContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Info | contracts/unstake-account/main.tolk:154 | `loadMessageFlags` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/unstake-account/serdes.tolk:29 | `storeMessageQueryId` is deprecated in Tolk's own standard library |
| DEPR | Info | contracts/unstake-account/serdes.tolk:28 | `storeMessageOp` is deprecated in Tolk's own standard library |
| DEPR | Low | contracts/unstake-account/storage.tolk:11 | `getContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |
| DEPR | Low | contracts/unstake-account/storage.tolk:22 | `setContractData` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain |

### Ton Whales nominator pool — Ton Whales Bounty Program

At `0553e1b6ddfc`, 17 files analysed.

| Detector | Severity | Where | What |
|---|---|---|---|
| LEP | Info | compiled/nominators.fc:1069 | function `op_update` reads `slice` parameter `in_msg` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | compiled/nominators.fc:1239 | function `op_elector_stake_response_fail` reads `slice` parameter `in_msg` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | compiled/nominators.fc:1726 | function `op_nominators` reads `slice` parameter `in_msg` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | sources/modules/op-common.fc:130 | function `op_update` reads `slice` parameter `in_msg` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | sources/modules/op-controller.fc:98 | function `op_elector_stake_response_fail` reads `slice` parameter `in_msg` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |
| LEP | Info | sources/modules/op-nominators.fc:1 | function `op_nominators` reads `slice` parameter `in_msg` but does not call `.end_parse()` on every path — trailing data the caller attached would be silently ignored |

