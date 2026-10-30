import "op-codes";

// NFT sale smart contract

@pure
fun minGasAmount(): int
    asm "1000000000 PUSHINT"; // 1 TON

@inline
fun loadData(): (address, address, address, int, cell) {
  var ds = contract.getData().beginParse();
  return 
    (ds.loadAddress(), // marketplace_address 
      ds.loadAddress(), // nft_address
      ds.loadAddress(),  // nft_owner_address
      ds.loadCoins(), // full_price
      ds.loadRef() // fees_cell
     );
}

@inline
fun loadFees(feesCell: cell): (int, address, int) {
  var ds = feesCell.beginParse();
  return 
    (ds.loadCoins(), // marketplace_fee,
      ds.loadAddress(), // royalty_address 
      ds.loadCoins() // royalty_amount
     );
}


@inline
fun saveData(marketplaceAddress: address, nftAddress: address, nftOwnerAddress: address, fullPrice: int, feesCell: cell) {
  contract.setData(beginCell()
    .storeAddress(marketplaceAddress)
    .storeAddress(nftAddress)
    .storeAddress(nftOwnerAddress)
    .storeCoins(fullPrice)
    .storeRef(feesCell)
    .endCell());
}

fun buy(myBalance: int, marketplaceAddress: address, nftAddress: address, nftOwnerAddress: address, fullPrice: int, feesCell: cell, msgValue: int, senderAddress: address, queryId: int) {
  assert(msgValue >= fullPrice + minGasAmount()) throw 450;

  var (marketplaceFee, royaltyAddress, royaltyAmount) = loadFees(feesCell);

  var ownerMsg = beginCell()
           .storeUint(0x10, 6) // nobounce
           .storeAddress(nftOwnerAddress)
           .storeCoins(fullPrice - marketplaceFee - royaltyAmount + (myBalance - msgValue))
           .storeUint(0, 1 + 4 + 4 + 64 + 32 + 1 + 1);

  sendRawMessage(ownerMsg.endCell(), 1);


  var royaltyMsg = beginCell()
           .storeUint(0x10, 6) // nobounce
           .storeAddress(royaltyAddress)
           .storeCoins(royaltyAmount)
           .storeUint(0, 1 + 4 + 4 + 64 + 32 + 1 + 1);

  sendRawMessage(royaltyMsg.endCell(), 1);


  var marketplaceMsg = beginCell()
           .storeUint(0x10, 6) // nobounce
           .storeAddress(marketplaceAddress)
           .storeCoins(marketplaceFee)
           .storeUint(0, 1 + 4 + 4 + 64 + 32 + 1 + 1);

  sendRawMessage(marketplaceMsg.endCell(), 1);

  var nftMsg = beginCell()
           .storeUint(0x18, 6) 
           .storeAddress(nftAddress)
           .storeCoins(0)
           .storeUint(0, 1 + 4 + 4 + 64 + 32 + 1 + 1)
           .storeUint(OP_TRANSFER(), 32)
           .storeUint(queryId, 64)
           .storeAddress(senderAddress) // new_owner_address
           .storeAddress(senderAddress) // response_address
           .storeInt(0, 1) // empty custom_payload
           .storeCoins(0) // forward amount to new_owner_address
           .storeInt(0, 1); // empty forward_payload


  sendRawMessage(nftMsg.endCell(), 128 + 32);
}

// in the future, use: fun onInternalMessage(in: InMessage) {
fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {
    var cs: slice = inMsgFull.beginParse();
    var flags: int = cs.loadUint(4);

    if ((flags & 1) != 0) {  // ignore all bounced messages
        return;
    }

    var senderAddress: address = cs.loadAddress();

    var (marketplaceAddress, nftAddress, nftOwnerAddress, fullPrice, feesCell) = loadData();

    var isInitialized = (nftOwnerAddress as slice).remainingBitsCount() > 2; // not initialized if null address

    if (!isInitialized) {
      
      if (senderAddress.bitsEqual(marketplaceAddress)) {
         return; // just accept coins on deploy
      }

      assert(senderAddress.bitsEqual(nftAddress)) throw 500;
      var op: int = inMsgBody.loadUint(32);
      assert(op == OP_OWNERSHIP_ASSIGNED()) throw 501;
      var queryId: int = inMsgBody.loadUint(64);
      var prevOwnerAddress: address = inMsgBody.loadAddress();

      saveData(marketplaceAddress, nftAddress, prevOwnerAddress, fullPrice, feesCell);

      return;

    }

    if (inMsgBody.isEmpty()) {
        buy(myBalance, marketplaceAddress, nftAddress, nftOwnerAddress, fullPrice, feesCell, msgValue, senderAddress, 0);
        return;
    }

    var op: int = inMsgBody.loadUint(32);
    var queryId: int = inMsgBody.loadUint(64);

    if (op == 1) { // just accept coins
        return;
    }

    if (op == 2) { // buy
     
      buy(myBalance, marketplaceAddress, nftAddress, nftOwnerAddress, fullPrice, feesCell, msgValue, senderAddress, queryId);

      return;

    }

    if (op == 3) { // cancel sale
         assert(msgValue >= minGasAmount()) throw 457;
         assert(senderAddress.bitsEqual(nftOwnerAddress) | senderAddress.bitsEqual(marketplaceAddress)) throw 458;

         var msg = beginCell()
           .storeUint(0x10, 6) // nobounce
           .storeAddress(nftAddress)
           .storeCoins(0)
           .storeUint(0, 1 + 4 + 4 + 64 + 32 + 1 + 1)
           .storeUint(OP_TRANSFER(), 32)
           .storeUint(queryId, 64) 
           .storeAddress(nftOwnerAddress) // new_owner_address
           .storeAddress(nftOwnerAddress) // response_address;
           .storeInt(0, 1) // empty custom_payload
           .storeCoins(0) // forward amount to new_owner_address
           .storeInt(0, 1); // empty forward_payload

        sendRawMessage(msg.endCell(), 128 + 32);

        return;
    }

    throw 0xffff;
    
}

fun onExternalMessage(inMsg: slice) {
}

get fun get_sale_data(): (slice, slice, slice, int, int, slice, int) {
  var (marketplaceAddress, nftAddress, nftOwnerAddress, fullPrice, feesCell) = loadData();
  var (marketplaceFee, royaltyAddress, royaltyAmount) = loadFees(feesCell);
  return (marketplaceAddress as slice, nftAddress as slice, nftOwnerAddress as slice, fullPrice, marketplaceFee, royaltyAddress as slice, royaltyAmount);
}
