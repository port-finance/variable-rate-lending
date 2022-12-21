#![cfg(feature = "test-bpf")]

use solana_program_test::*;
use solana_sdk::signature::Keypair;

use helpers::*;
use port_finance_variable_rate_lending::processor::process_instruction;

mod helpers;

#[tokio::test]
async fn test_success() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(80_000);

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_pyth_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: 100 * FRACTIONAL_TO_USDC,
            liquidity_amount: 10_000 * FRACTIONAL_TO_USDC,
            liquidity_mint_decimals: usdc_mint.decimals,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            config: TEST_RESERVE_CONFIG,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, _recent_blockhash) = test.start().await;
    let init_liquidity = usdc_test_reserve
        .get_state(&mut banks_client)
        .await
        .liquidity
        .available_amount;
    lending_market
        .deposit(
            &mut banks_client,
            &user_accounts_owner,
            &payer,
            &usdc_test_reserve,
            100 * FRACTIONAL_TO_USDC,
        )
        .await;

    assert_eq!(
        usdc_test_reserve
            .get_state(&mut banks_client)
            .await
            .liquidity
            .available_amount,
        init_liquidity + 100 * FRACTIONAL_TO_USDC
    )
}
