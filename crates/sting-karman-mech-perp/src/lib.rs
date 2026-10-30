//! Order-driven perpetual-futures mechanics: the family where price is endogenous — it forms in
//! a central limit order book — and the mechanism is margin: cross-margined positions, funding
//! transfers, and forced deleveraging when equity runs out.
//!
//! The reference venue is Hyperliquid, whose risk-engine semantics are public and whose L4
//! order data is the replay material for levels 1 and 2 of the verification ladder
//! (karman/docs/VERIFICATION.md). Of the three mechanism families this is the deepest: matching,
//! margin and liquidation interact, and the order book makes the state large.
