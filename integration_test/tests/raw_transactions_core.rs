// SPDX-License-Identifier: CC0-1.0

//! Tests derived from Bitcoin Core's rpc_rawtransaction.py, rpc_psbt.py and rpc_decodescript.py

#[cfg(not(feature = "v21_and_below"))]
use bitcoin::PublicKey;
#[cfg(not(feature = "v20_and_below"))]
use bitcoin::Transaction;
#[cfg(not(feature = "v17"))]
use bitcoin::{Amount, Psbt};
use bitcoind::vtype::*;
#[cfg(not(feature = "v17"))]
use bitcoind::{Input, Output};
use integration_test::{BitcoinD, BitcoinDExt as _, Wallet};

#[test]
fn get_raw_transaction_verbose_mined_has_block_hash() {
    let node = BitcoinD::with_wallet(Wallet::Default, &["-txindex"]);
    node.fund_wallet();
    let (_, tx) = node.create_mined_transaction();

    let json: GetRawTransactionVerbose =
        node.client.get_raw_transaction_verbose(tx.compute_txid()).unwrap();

    assert!(json.block_hash.is_some());
}

#[test]
#[cfg(not(feature = "v20_and_below"))]
fn test_mempool_accept_allowed_has_optional_fields() {
    let node = BitcoinD::with_wallet(Wallet::Default, &[]);
    node.fund_wallet();

    let signed = build_and_sign_unbroadcast_tx(&node);

    let json: TestMempoolAccept =
        node.client.test_mempool_accept(std::slice::from_ref(&signed)).unwrap();

    let res = &json.0[0];
    assert!(res.allowed);
    assert!(res.vsize.is_some());
    assert!(res.fees.is_some());
}

#[test]
#[cfg(not(feature = "v21_and_below"))]
fn test_mempool_accept_rejected_has_reason() {
    let node = BitcoinD::with_wallet(Wallet::Default, &["-txindex"]);
    node.fund_wallet();

    let (_, txid) = node.create_mempool_transaction();
    let tx = node.client.get_raw_transaction(txid).unwrap().transaction().unwrap();

    let json: TestMempoolAccept =
        node.client.test_mempool_accept(std::slice::from_ref(&tx)).unwrap();

    let res = &json.0[0];
    assert!(!res.allowed);
    assert!(res.reject_reason.is_some());
}

#[test]
#[cfg(not(feature = "v17"))]
fn analyze_psbt_has_estimates_after_wallet_process() {
    let node = BitcoinD::with_wallet(Wallet::Default, &[]);
    node.fund_wallet();

    let psbt = build_psbt(&node);
    let processed: WalletProcessPsbt = node.client.wallet_process_psbt(&psbt).unwrap();

    let parsed: Psbt = processed.psbt.parse().unwrap();
    let json: AnalyzePsbt = node.client.analyze_psbt(&parsed).unwrap();

    assert!(json.estimated_vsize.is_some());
    assert!(json.estimated_fee_rate.is_some());
    assert!(json.fee.is_some());
}

#[test]
#[cfg(not(feature = "v19_and_below"))]
fn decode_psbt_has_witness_utxo_after_utxo_update() {
    let node = BitcoinD::with_wallet(Wallet::Default, &[]);
    node.fund_wallet();

    let psbt = build_psbt(&node);
    let updated: UtxoUpdatePsbt = node.client.utxo_update_psbt(&psbt).unwrap();
    let updated_psbt = updated.into_model().unwrap().0;

    let json: DecodePsbt = node.client.decode_psbt(&updated_psbt.to_string()).unwrap();

    assert!(json.inputs.iter().all(|i| i.witness_utxo.is_some() ^ i.non_witness_utxo.is_some()));
}

#[test]
#[cfg(not(feature = "v19_and_below"))]
fn decode_psbt_input_final_witness_after_signing() {
    let node = BitcoinD::with_wallet(Wallet::Default, &[]);
    node.fund_wallet();

    let psbt = build_psbt(&node);
    let processed: WalletProcessPsbt = node.client.wallet_process_psbt(&psbt).unwrap();
    assert!(processed.complete);

    let json: DecodePsbt = node.client.decode_psbt(&processed.psbt).unwrap();

    assert!(json.inputs.iter().all(|i| i.final_script_witness.is_some()));
}

#[test]
#[cfg(not(feature = "v22_and_below"))]
fn decode_psbt_input_bip32_derivs_for_unfinalized_psbt() {
    let node = BitcoinD::with_wallet(Wallet::Default, &[]);
    node.fund_wallet();

    let psbt = build_psbt(&node);

    let processed: WalletProcessPsbt = node
        .client
        .call(
            "walletprocesspsbt",
            &[
                bitcoind::serde_json::json!(psbt.to_string()),
                bitcoind::serde_json::json!(true),
                bitcoind::serde_json::json!("DEFAULT"),
                bitcoind::serde_json::json!(true),
                bitcoind::serde_json::json!(false),
            ],
        )
        .unwrap();

    let json: DecodePsbt = node.client.decode_psbt(&processed.psbt).unwrap();

    assert!(json.inputs.iter().all(|i| i.bip32_derivs.is_some()));
}

#[test]
#[cfg(not(feature = "v21_and_below"))]
fn decode_script_multisig_has_segwit_and_p2sh_segwit() {
    let node = BitcoinD::with_wallet(Wallet::Default, &[]);

    let pk1: PublicKey =
        "02ff12471208c14bd580709cb2358d98975247d8765f92bc25eab3b2763ed605f8".parse().unwrap();
    let pk2: PublicKey =
        "02fe6f0a5a297eb38c391581c4413e084773ea23954d93f7753db7dc0adc188b2f".parse().unwrap();
    let multisig: CreateMultisig = node.client.create_multisig(1, vec![pk1, pk2]).unwrap();

    let json: DecodeScript = node.client.decode_script(&multisig.redeem_script).unwrap();

    let segwit = json.segwit.as_ref().unwrap();
    assert!(segwit.p2sh_segwit.is_some());
}

#[test]
#[cfg(not(feature = "v22_and_below"))]
fn decode_script_multisig_has_descriptor() {
    let node = BitcoinD::with_wallet(Wallet::Default, &[]);

    let pk1: PublicKey =
        "02ff12471208c14bd580709cb2358d98975247d8765f92bc25eab3b2763ed605f8".parse().unwrap();
    let pk2: PublicKey =
        "02fe6f0a5a297eb38c391581c4413e084773ea23954d93f7753db7dc0adc188b2f".parse().unwrap();
    let multisig: CreateMultisig = node.client.create_multisig(1, vec![pk1, pk2]).unwrap();

    let json: DecodeScript = node.client.decode_script(&multisig.redeem_script).unwrap();

    assert!(json.descriptor.is_some());
}

#[test]
fn get_raw_transaction_verbose_mined_has_time_fields() {
    let node = BitcoinD::with_wallet(Wallet::Default, &["-txindex"]);
    node.fund_wallet();
    let (_, tx) = node.create_mined_transaction();

    let json: GetRawTransactionVerbose =
        node.client.get_raw_transaction_verbose(tx.compute_txid()).unwrap();

    assert!(json.transaction_time.is_some());
    assert!(json.block_time.is_some());
}

#[test]
fn get_raw_transaction_verbose_with_blockhash_is_in_active_chain() {
    let node = BitcoinD::with_wallet(Wallet::Default, &["-txindex"]);
    node.fund_wallet();
    let (_, tx) = node.create_mined_transaction();
    let txid = tx.compute_txid();

    let best = node.client.best_block_hash().unwrap();

    let json: GetRawTransactionVerbose = node
        .client
        .call(
            "getrawtransaction",
            &[
                bitcoind::serde_json::json!(txid.to_string()),
                bitcoind::serde_json::json!(true),
                bitcoind::serde_json::json!(best.to_string()),
            ],
        )
        .unwrap();

    assert_eq!(json.in_active_chain, Some(true));
}

#[cfg(not(feature = "v17"))]
fn build_inputs_outputs(node: &BitcoinD) -> (Vec<Input>, Vec<Output>) {
    let (_, tx) = node.create_mined_transaction();
    let txid = tx.compute_txid();

    let million = Amount::from_sat(1_000_000);
    let (vout, value) = {
        let v0 = node.client.get_tx_out(txid, 0).unwrap().into_model().unwrap();
        if v0.tx_out.value == million {
            (0u64, v0.tx_out.value)
        } else {
            let v1 = node.client.get_tx_out(txid, 1).unwrap().into_model().unwrap();
            (1u64, v1.tx_out.value)
        }
    };

    let spend = Amount::from_sat(100_000);
    let fee = Amount::from_sat(1_000);
    let change = value - spend - fee;

    let inputs = vec![Input { txid, vout, sequence: None }];
    let spend_addr = node.client.new_address().unwrap();
    let change_addr =
        node.client.get_raw_change_address().unwrap().into_model().unwrap().0.assume_checked();
    let outputs = vec![Output::new(spend_addr, spend), Output::new(change_addr, change)];

    (inputs, outputs)
}

#[cfg(not(feature = "v17"))]
fn build_psbt(node: &BitcoinD) -> Psbt {
    let (inputs, outputs) = build_inputs_outputs(node);
    let json: CreatePsbt = node.client.create_psbt(&inputs, &outputs).unwrap();
    json.into_model().unwrap().0
}

#[cfg(not(feature = "v20_and_below"))]
fn build_and_sign_unbroadcast_tx(node: &BitcoinD) -> Transaction {
    let (inputs, outputs) = build_inputs_outputs(node);
    let json: CreateRawTransaction = node.client.create_raw_transaction(&inputs, &outputs).unwrap();
    let raw = json.transaction().unwrap();

    let signed: SignRawTransactionWithWallet =
        node.client.sign_raw_transaction_with_wallet(&raw).unwrap();
    signed.into_model().unwrap().tx
}
