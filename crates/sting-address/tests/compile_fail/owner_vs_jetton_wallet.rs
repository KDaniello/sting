use sting_address::{JettonWalletAddr, OwnerId, TonAddress};

/// A funding edge runs owner to owner. A jetton wallet reaching this shape is
/// the confusion the two newtypes exist to prevent.
struct FundingEdge {
    src: OwnerId,
    dst: OwnerId,
}

fn main() {
    let owner = OwnerId(TonAddress::new(0, [0u8; 32]));
    let jetton_wallet = JettonWalletAddr(TonAddress::new(0, [1u8; 32]));

    let _edge = FundingEdge {
        src: jetton_wallet,
        dst: owner,
    };
}
