//! The invariant that only the compiler can hold: an owner and a jetton wallet
//! are not interchangeable.
//!
//! No `.stderr` snapshot: what is under test is that the code does not compile,
//! not the wording rustc refuses it with — a wording that changes with the
//! toolchain and would fail this test for a reason that is not a defect.

#[test]
fn a_jetton_wallet_cannot_be_passed_where_an_owner_is_expected() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/*.rs");
}
