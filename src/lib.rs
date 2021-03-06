//! This crate is an experimental implementation of zkChannels
//! - formerly Blind Off-chain lightweight transactions (BOLT).
//!
//! zkChannels extends academic work by Matthew Green and Ian Miers -
//! https://eprint.iacr.org/2016/701.
//!
//! Libzkchannels relies on the EMP-toolkit (['emp-ag2pc`](https://github.com/boltlabs-inc/emp-ag2pc) and
//! ['emp-sh2pc`](https://github.com/boltlabs-inc/emp-sh2pc)), BN-256 and BLS12-381 curves at 128-bit security,
//! as implemented in a fork of [`pairing module`](https://github.com/boltlabs-inc/pairing).
//!
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(unused_parens)]
#![allow(non_upper_case_globals)]
#![allow(unused_results)]
#![allow(missing_docs)]
#![cfg_attr(all(test, feature = "unstable"), feature(test))]
#[cfg(all(test, feature = "unstable"))]
extern crate test;

extern crate ff_bl as ff;
extern crate pairing_bl as pairing;
extern crate rand;

extern crate secp256k1;
extern crate sha2;
extern crate time;

extern crate serde;
extern crate serde_with;

extern crate hex;
extern crate libc;

extern crate bit_array;
extern crate hmac;
extern crate num;
extern crate ripemd160;
extern crate serde_json;
extern crate typenum;

extern crate redis;
extern crate zkchan_tx;

#[macro_use]
extern crate enum_display_derive;

#[cfg(test)]
#[macro_use]
extern crate rusty_fork;
extern crate rand_xorshift;
extern crate serde_bytes;

pub mod bindings;
pub mod ccs08;
pub mod channels;
pub mod channels_mpc;
pub mod cl;
pub mod database;
pub mod ecdsa_partial;
pub mod ffishim_bls12;
pub mod ffishim_bn256;
pub mod ffishim_mpc;
pub mod mpcwrapper;
pub mod nizk;
pub mod ped92;
pub mod util;
pub mod wallet;

#[cfg(test)]
pub mod test_e2e;

use ff::{Field, Rand};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str;
use zkchan_tx::fixed_size_array::FixedSizeArray32;

////////////////////////////////// Utilities //////////////////////////////////

pub type BoltResult<T> = Result<Option<T>, String>;

#[macro_export]
macro_rules! handle_bolt_result {
    ($e:expr) => {
        match $e {
            Ok(val) => val,
            Err(_) => None,
        }
    };
}

////////////////////////////////// Utilities //////////////////////////////////

/////////////////////////////// ZKproofs ////////////////////////////////
pub mod zkproofs {
    use cl;
    use pairing::Engine;
    use rand::Rng;
    use util;
    use wallet;
    // for blind signature
    use secp256k1;
    // for on-chain keys
    use HashMap;

    pub use channels::{
        BoltError, ChannelParams, ChannelState, ChannelToken, ChannelcloseM, CustomerState,
        MerchantState, PubKeyMap, ResultBoltType,
    };
    pub use cl::PublicParams;
    pub use cl::{PublicKey, Signature};
    pub use nizk::NIZKProof;
    pub use ped92::Commitment;
    pub use ped92::CommitmentProof;
    use serde::{Deserialize, Serialize};
    use util::{hash_to_slice, RevokedMessage};
    pub use wallet::{serialize_compact, Wallet};
    pub use BoltResult;

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(bound(serialize = "<E as ff::ScalarEngine>::Fr: serde::Serialize, \
                               <E as pairing::Engine>::G1: serde::Serialize"))]
    #[serde(
        bound(deserialize = "<E as ff::ScalarEngine>::Fr: serde::Deserialize<'de>, \
                             <E as pairing::Engine>::G1: serde::Deserialize<'de>")
    )]
    pub struct ChannelcloseC<E: Engine> {
        pub wpk: secp256k1::PublicKey,
        pub message: wallet::Wallet<E>,
        pub merch_signature: cl::Signature<E>,
        pub cust_signature: secp256k1::Signature,
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(bound(serialize = "<E as ff::ScalarEngine>::Fr: serde::Serialize, \
                               <E as pairing::Engine>::G1: serde::Serialize, \
                               <E as pairing::Engine>::G2: serde::Serialize, \
                               <E as pairing::Engine>::Fqk: serde::Serialize"))]
    #[serde(
        bound(deserialize = "<E as ff::ScalarEngine>::Fr: serde::Deserialize<'de>, \
                             <E as pairing::Engine>::G1: serde::Deserialize<'de>, \
                             <E as pairing::Engine>::G2: serde::Deserialize<'de>,\
                             <E as pairing::Engine>::Fqk: serde::Deserialize<'de>")
    )]
    pub struct Payment<E: Engine> {
        proof: NIZKProof<E>,
        com: Commitment<E>,
        wpk: secp256k1::PublicKey,
        amount: i64,
    }

    #[derive(Clone, Serialize, Deserialize)]
    pub struct RevokeToken {
        message: util::RevokedMessage,
        pub signature: secp256k1::Signature,
    }

    ///
    /// init_merchant - takes as input the public params, merchant balance and keypair.
    /// Generates merchant data which consists of channel token and merchant state.
    ///
    pub fn init_merchant<'a, R: Rng, E: Engine>(
        csprng: &mut R,
        channel_state: &mut ChannelState<E>,
        name: &'a str,
    ) -> (ChannelToken<E>, MerchantState<E>, ChannelState<E>) {
        // create new merchant state
        let merch_name = String::from(name);
        let (mut merch_state, mut channel_state) =
            MerchantState::<E>::new(csprng, channel_state, merch_name);
        // initialize the merchant state
        let channel_token = merch_state.init(&mut channel_state);

        return (channel_token, merch_state, channel_state.clone());
    }

    ///
    /// init_customer - takes as input the public params, channel state, commitment params, keypair,
    /// and initial balance for customer and merchant. Generate initial customer channel token,
    /// and wallet commitment.
    ///
    pub fn init_customer<'a, R: Rng, E: Engine>(
        csprng: &mut R,
        channel_token: &mut ChannelToken<E>,
        b0_cust: i64,
        b0_merch: i64,
        name: &'a str,
    ) -> CustomerState<E>
    where
        <E as pairing::Engine>::G1: serde::Serialize,
        <E as pairing::Engine>::G2: serde::Serialize,
        <E as ff::ScalarEngine>::Fr: serde::Serialize,
    {
        assert!(b0_cust >= 0);
        assert!(b0_merch >= 0);

        let cust_name = String::from(name);
        return CustomerState::<E>::new(csprng, channel_token, b0_cust, b0_merch, cust_name);
    }

    ///
    /// establish_customer_generate_proof (Phase 1) - takes as input the public params, customer state and
    /// common public bases from merchant. Generates a PoK of the committed values in the
    /// new wallet.
    ///
    pub fn establish_customer_generate_proof<R: Rng, E: Engine>(
        csprng: &mut R,
        channel_token: &ChannelToken<E>,
        cust_state: &CustomerState<E>,
    ) -> (Commitment<E>, CommitmentProof<E>) {
        let cust_com_proof = cust_state.generate_proof(csprng, channel_token);
        return (cust_state.w_com.clone(), cust_com_proof);
    }

    ///
    /// establish_merchant_issue_close_token (Phase 1) - takes as input the channel state,
    /// PoK of committed values from the customer. Generates close token (a blinded
    /// signature) over the contents of the customer's wallet.
    ///
    pub fn establish_merchant_issue_close_token<R: Rng, E: Engine>(
        csprng: &mut R,
        channel_state: &ChannelState<E>,
        com: &Commitment<E>,
        com_proof: &CommitmentProof<E>,
        channel_id: &E::Fr,
        init_cust_balance: i64,
        init_merch_balance: i64,
        merch_state: &MerchantState<E>,
    ) -> BoltResult<cl::Signature<E>> {
        // verifies proof of committed values and derives blind signature on the committed values to the customer's initial wallet
        match merch_state.verify_proof(
            csprng,
            channel_state,
            com,
            com_proof,
            channel_id,
            init_cust_balance,
            init_merch_balance,
        ) {
            Ok(n) => Ok(Some(n.0)), // just close token
            Err(err) => Err(String::from(err.to_string())),
        }
    }

    ///
    /// establish_merchant_issue_pay_token (Phase 1) - takes as input the channel state,
    /// the commitment from the customer. Generates close token (a blinded
    /// signature) over the contents of the customer's wallet.
    ///
    pub fn establish_merchant_issue_pay_token<R: Rng, E: Engine>(
        csprng: &mut R,
        channel_state: &ChannelState<E>,
        com: &Commitment<E>,
        merch_state: &MerchantState<E>,
    ) -> cl::Signature<E> {
        let cp = channel_state.cp.as_ref().unwrap();
        let pay_token = merch_state.issue_pay_token(csprng, cp, com, false);
        return pay_token;
    }

    ///
    /// establish_customer_final - takes as input the channel state, customer state,
    /// and pay token (blinded sig) obtained from merchant. Add the returned
    /// blinded signature to the wallet.
    ///
    pub fn establish_customer_final<E: Engine>(
        channel_state: &mut ChannelState<E>,
        cust_state: &mut CustomerState<E>,
        pay_token: &cl::Signature<E>,
    ) -> bool {
        // verify the pay-token first
        if !cust_state.verify_pay_token(&channel_state, pay_token) {
            println!("establish_customer_final - Failed to verify the pay-token");
            return false;
        }

        // only if both tokens have been stored
        if (cust_state.has_tokens()) {
            // must be an old wallet
            channel_state.channel_established = true;
        }
        return channel_state.channel_established;
    }
    ///// end of establish channel protocol

    ///
    /// generate_payment_proof (phase 1) - takes as input the public params, channel state, channel token,
    /// merchant public keys, old wallet and balance increment. Generate a new wallet commitment
    /// PoK of the committed values in new wallet and PoK of old wallet. Return new channel token,
    /// new wallet (minus blind signature and refund token) and payment proof.
    ///
    pub fn generate_payment_proof<R: Rng, E: Engine>(
        csprng: &mut R,
        channel_state: &ChannelState<E>,
        cust_state: &CustomerState<E>,
        amount: i64,
    ) -> (Payment<E>, CustomerState<E>) {
        let tx_fee = channel_state.get_channel_fee();
        let payment_amount = match tx_fee > 0 {
            true => amount + tx_fee,
            false => amount,
        };
        let (proof, com, wpk, new_cust_state) =
            cust_state.generate_payment(csprng, &channel_state, payment_amount);
        let payment = Payment {
            proof,
            com,
            wpk,
            amount,
        };
        return (payment, new_cust_state);
    }

    ///
    /// verify_payment (phase 1) - takes as input the public params, channel state, payment proof
    /// and merchant keys. If proof is valid, then merchant returns the refund token
    /// (i.e., partially blind signature on IOU with updated balance)
    ///
    pub fn verify_payment_proof<R: Rng, E: Engine>(
        csprng: &mut R,
        channel_state: &ChannelState<E>,
        payment: &Payment<E>,
        merch_state: &mut MerchantState<E>,
    ) -> cl::Signature<E> {
        // if payment proof verifies, then returns close-token and records wpk => pay-token
        // if valid revoke_token is provided later for wpk, then release pay-token
        let tx_fee = channel_state.get_channel_fee();
        let payment_amount = match tx_fee > 0 {
            true => payment.amount + tx_fee,
            false => payment.amount,
        };
        let new_close_token = merch_state
            .verify_payment(
                csprng,
                &channel_state,
                &payment.proof,
                &payment.com,
                &payment.wpk,
                payment_amount,
            )
            .unwrap();
        // store the wpk since it has been revealed
        update_merchant_state(&mut merch_state.keys, &payment.wpk, None);
        return new_close_token;
    }

    ///
    /// Verify third party payment proof from two bi-directional channel payments with intermediary (payment amount
    ///
    pub fn verify_multiple_payment_proofs<R: Rng, E: Engine>(
        csprng: &mut R,
        channel_state: &ChannelState<E>,
        sender_payment: &Payment<E>,
        receiver_payment: &Payment<E>,
        merch_state: &mut MerchantState<E>,
    ) -> BoltResult<(cl::Signature<E>, cl::Signature<E>)> {
        let tx_fee = channel_state.get_channel_fee();
        let amount = sender_payment.amount + receiver_payment.amount;
        if amount != 0 {
            // we want to check this relation in ZK without knowing the amount
            return Err(String::from("payments do not offset"));
        }

        let new_close_token = merch_state
            .verify_payment(
                csprng,
                &channel_state,
                &sender_payment.proof,
                &sender_payment.com,
                &sender_payment.wpk,
                sender_payment.amount + tx_fee,
            )
            .unwrap();

        let cond_close_token = merch_state
            .verify_payment(
                csprng,
                &channel_state,
                &receiver_payment.proof,
                &receiver_payment.com,
                &receiver_payment.wpk,
                receiver_payment.amount + tx_fee,
            )
            .unwrap();

        // store the wpk since it has been revealed
        update_merchant_state(&mut merch_state.keys, &sender_payment.wpk, None);
        update_merchant_state(&mut merch_state.keys, &receiver_payment.wpk, None);

        return Ok(Some((new_close_token, cond_close_token)));
    }

    ///
    /// generate_revoke_token (phase 2) - takes as input the public params, old wallet, new wallet,
    /// merchant's verification key and refund token. If the refund token is valid, generate
    /// a revocation token for the old wallet public key.
    ///
    pub fn generate_revoke_token<E: Engine>(
        channel_state: &ChannelState<E>,
        old_cust_state: &mut CustomerState<E>,
        new_cust_state: CustomerState<E>,
        new_close_token: &cl::Signature<E>,
    ) -> RevokeToken {
        // let's update the old wallet
        assert!(old_cust_state.update(new_cust_state));
        // generate the token after verifying that the close token is valid
        let (message, signature) = old_cust_state
            .generate_revoke_token(channel_state, new_close_token)
            .unwrap();
        // return the revoke token (msg + sig pair)
        return RevokeToken { message, signature };
    }

    ///
    /// verify_revoke_token (phase 2) - takes as input revoke message and signature
    /// from the customer and the merchant state. If the revocation token is valid,
    /// generate a new signature for the new wallet (from the PoK of committed values in new wallet).
    ///
    pub fn verify_revoke_token<E: Engine>(
        rt: &RevokeToken,
        merch_state: &mut MerchantState<E>,
    ) -> BoltResult<cl::Signature<E>> {
        let pay_token_result =
            merch_state.verify_revoke_token(&rt.signature, &rt.message, &rt.message.wpk);
        let new_pay_token = match pay_token_result {
            Ok(n) => n,
            Err(err) => return Err(String::from(err.to_string())),
        };
        update_merchant_state(
            &mut merch_state.keys,
            &rt.message.wpk,
            Some(rt.signature.clone()),
        );
        Ok(Some(new_pay_token))
    }

    ///
    /// verify_multiple_revoke_tokens (phase 2) - takes as input revoke messages and signatures
    /// from the sender and receiver and the merchant state of the intermediary.
    /// If the revocation tokens are valid, generate new signatures for the new wallets of both
    /// sender and receiver (from the PoK of committed values in new wallet).
    ///
    pub fn verify_multiple_revoke_tokens<E: Engine>(
        rt_sender: &RevokeToken,
        rt_receiver: &RevokeToken,
        merch_state: &mut MerchantState<E>,
    ) -> BoltResult<(cl::Signature<E>, cl::Signature<E>)> {
        let pay_token_sender_result = merch_state.verify_revoke_token(
            &rt_sender.signature,
            &rt_sender.message,
            &rt_sender.message.wpk,
        );
        let pay_token_receiver_result = merch_state.verify_revoke_token(
            &rt_receiver.signature,
            &rt_receiver.message,
            &rt_receiver.message.wpk,
        );
        let new_pay_token_sender = match pay_token_sender_result {
            Ok(n) => n,
            Err(err) => return Err(String::from(err.to_string())),
        };
        let new_pay_token_receiver = match pay_token_receiver_result {
            Ok(n) => n,
            Err(err) => return Err(String::from(err.to_string())),
        };

        update_merchant_state(
            &mut merch_state.keys,
            &rt_sender.message.wpk,
            Some(rt_sender.signature.clone()),
        );
        update_merchant_state(
            &mut merch_state.keys,
            &rt_receiver.message.wpk,
            Some(rt_receiver.signature.clone()),
        );

        Ok(Some((new_pay_token_sender, new_pay_token_receiver)))
    }

    ///// end of pay protocol

    // for customer => on input a wallet w, it outputs a customer channel closure message
    ///
    /// customer_close - takes as input the channel state, merchant's verification
    /// key, and customer state. Generates a channel closure message for customer.
    ///
    pub fn customer_close<E: Engine>(
        channel_state: &ChannelState<E>,
        cust_state: &CustomerState<E>,
    ) -> ChannelcloseC<E>
    where
        <E as pairing::Engine>::G1: serde::Serialize,
    {
        if !channel_state.channel_established {
            panic!("Cannot close a channel that has not been established!");
        }

        let mut wallet = cust_state.get_wallet();
        let close_token = cust_state.get_close_token();

        let cp = channel_state.cp.as_ref().unwrap();
        let pk = cp.pub_params.pk.get_pub_key();
        let close_wallet = wallet.with_close(String::from("close"));

        assert!(pk.verify(&cp.pub_params.mpk, &close_wallet, &close_token));

        // hash the closing wallet + close token (merch sig)
        let mut m1 = serialize_compact::<E>(&close_wallet);
        let m2 = close_token.serialize_compact();
        m1.extend_from_slice(&m2);
        let m = hash_to_slice(&m2);

        // compute secp256k1 signature on the hash
        let secp = secp256k1::Secp256k1::new();
        let msg = secp256k1::Message::from_slice(&m).unwrap();
        let seckey = cust_state.get_secret_key();
        let cust_sig = secp.sign(&msg, &seckey);

        ChannelcloseC {
            wpk: cust_state.wpk,
            message: wallet,
            merch_signature: close_token,
            cust_signature: cust_sig,
        }
    }

    fn update_merchant_state(
        db: &mut HashMap<String, PubKeyMap>,
        wpk: &secp256k1::PublicKey,
        rev: Option<secp256k1::Signature>,
    ) {
        let fingerprint = util::compute_pub_key_fingerprint(wpk);
        //println!("Print fingerprint: {}", fingerprint);
        if !rev.is_none() {
            let cust_pub_key = PubKeyMap {
                wpk: wpk.clone(),
                revoke_token: Some(rev.unwrap().clone()),
            };
            db.insert(fingerprint, cust_pub_key);
        } else {
            let cust_pub_key = PubKeyMap {
                wpk: wpk.clone(),
                revoke_token: None,
            };
            db.insert(fingerprint, cust_pub_key);
        }
    }

    ///
    /// merchant_close - takes as input the channel state, channel token, customer close msg/sig,
    /// Returns tokens for merchant close transaction (only if customer close message is found to be a
    /// double spend). If not, then None is returned.
    ///
    pub fn merchant_close<E: Engine>(
        channel_state: &ChannelState<E>,
        channel_token: &ChannelToken<E>,
        cust_close: &ChannelcloseC<E>,
        merch_state: &MerchantState<E>,
    ) -> BoltResult<PubKeyMap> {
        if (!channel_state.channel_established) {
            return Err(String::from("merchant_close - Channel not established! Cannot generate channel closure message."));
        }

        let cp = channel_state.cp.as_ref().unwrap();
        let pk = cp.pub_params.pk.get_pub_key();
        let mut wallet = cust_close.message.clone();
        let close_wallet = wallet.with_close(String::from("close")).clone();
        let close_token = cust_close.merch_signature.clone();

        let is_valid = pk.verify(&channel_token.mpk, &close_wallet, &close_token);

        if is_valid {
            let wpk = cust_close.wpk;
            // found the wpk, which means old close token
            let fingerprint = util::compute_pub_key_fingerprint(&wpk);
            if merch_state.keys.contains_key(&fingerprint) {
                let revoked_state = merch_state.keys.get(&fingerprint).unwrap();
                if !revoked_state.revoke_token.is_none() {
                    let revoke_token = revoked_state.revoke_token.unwrap().clone();
                    // verify the revoked state first before returning
                    let secp = secp256k1::Secp256k1::new();
                    let revoke_msg = RevokedMessage::new(String::from("revoked"), wpk.clone());
                    let msg = secp256k1::Message::from_slice(&revoke_msg.hash_to_slice()).unwrap();
                    // verify that the revocation token is valid
                    if secp.verify(&msg, &revoke_token, &wpk).is_ok() {
                        // compute signature on
                        return Ok(Some(revoked_state.clone()));
                    }
                }
                return Err(String::from("merchant_close - Found wpk but could not find the revoke token. Merchant abort detected."));
            }
            return Err(String::from(
                "merchant_close - Could not find entry for wpk & revoke token pair. Valid close!",
            ));
        }
        Err(String::from(
            "merchant_close - Customer close message not valid!",
        ))
    }

    ///
    /// Used in open-channel WTP for validating that a close_token is a valid signature under <
    ///
    pub fn tze_verify_cust_close_message<E: Engine>(
        channel_token: &ChannelToken<E>,
        wpk: &secp256k1::PublicKey,
        close_msg: &wallet::Wallet<E>,
        close_token: &Signature<E>,
    ) -> bool {
        // close_msg => <pkc> || <wpk> || <balance-cust> || <balance-merch> || CLOSE
        // close_token = regular CL signature on close_msg
        // channel_token => <pk_c, CL_PK_m, pk_m, mpk, comParams>

        // (1) check that channel token and close msg are consistent (e.g., close_msg.pk_c == H(channel_token.pk_c) &&
        let pk_c = channel_token.pk_c.unwrap();
        let chan_token_pk_c = util::hash_pubkey_to_fr::<E>(&pk_c);
        let chan_token_wpk = util::hash_pubkey_to_fr::<E>(&wpk);

        let pkc_thesame = (close_msg.channelId == chan_token_pk_c);
        // (2) check that wpk matches what's in the close msg
        let wpk_thesame = (close_msg.wpk == chan_token_wpk);
        return pkc_thesame
            && wpk_thesame
            && channel_token.cl_pk_m.verify(
                &channel_token.mpk,
                &close_msg.as_fr_vec(),
                &close_token,
            );
    }

    ///
    /// Used in merch-close WTP for validating that revoke_token is a valid signature under <wpk> and the <revoked || wpk> message
    ///
    pub fn tze_verify_revoke_message(
        wpk: &secp256k1::PublicKey,
        revoke_token: &secp256k1::Signature,
    ) -> bool {
        let secp = secp256k1::Secp256k1::verification_only();
        let revoke_msg = RevokedMessage::new(String::from("revoked"), wpk.clone());
        let msg = secp256k1::Message::from_slice(&revoke_msg.hash_to_slice()).unwrap();
        // verify that the revocation token is valid with respect to revoked || wpk
        return secp.verify(&msg, &revoke_token, &wpk).is_ok();
    }

    ///
    /// Used in merch-close WTP for validating that merch_sig is a valid signature under <merch_pk> on <dest_addr || revoke-token> message
    ///
    pub fn tze_verify_merch_close_message<E: Engine>(
        channel_token: &ChannelToken<E>,
        merch_close: &ChannelcloseM,
    ) -> bool {
        let secp = secp256k1::Secp256k1::verification_only();
        let mut msg = Vec::new();
        msg.extend(merch_close.address.as_bytes());
        if !merch_close.revoke.is_none() {
            // serialize signature in DER format
            let r = merch_close.revoke.unwrap().serialize_der().to_vec();
            msg.extend(r);
        }
        let msg2 = secp256k1::Message::from_slice(&hash_to_slice(&msg)).unwrap();
        // verify that merch sig is valid with respect to dest_address
        return secp
            .verify(&msg2, &merch_close.signature, &channel_token.pk_m)
            .is_ok();
    }
}

pub mod tze_utils {
    // Useful routines that simplify the Bolt WTP implementation for Zcash
    pub use channels::ChannelToken;
    use channels::ChannelcloseM;
    use cl;
    pub use cl::Signature;
    use pairing::bls12_381::Bls12;
    use ped92::CSMultiParams;
    pub use wallet::Wallet;
    use {util, BoltResult};

    const BLS12_381_CHANNEL_TOKEN_LEN: usize = 1074;
    const BLS12_381_G1_LEN: usize = 48;
    const BLS12_381_G2_LEN: usize = 96;
    const SECP256K1_PK_LEN: usize = 33;
    const ADDRESS_LEN: usize = 33;

    pub fn reconstruct_secp_public_key(pk_bytes: &[u8; SECP256K1_PK_LEN]) -> secp256k1::PublicKey {
        return secp256k1::PublicKey::from_slice(pk_bytes).unwrap();
    }

    pub fn reconstruct_secp_signature(sig_bytes: &[u8]) -> secp256k1::Signature {
        return secp256k1::Signature::from_der(sig_bytes).unwrap();
    }

    pub fn reconstruct_close_wallet_bls12(
        channel_token: &ChannelToken<Bls12>,
        wpk: &secp256k1::PublicKey,
        cust_bal: u32,
        merch_bal: u32,
    ) -> Wallet<Bls12> {
        let channelId = channel_token.compute_channel_id();
        let wpk_h = util::hash_pubkey_to_fr::<Bls12>(&wpk);
        let close = util::hash_to_fr::<Bls12>(String::from("close").into_bytes());

        return Wallet {
            channelId,
            wpk: wpk_h,
            bc: cust_bal as i64,
            bm: merch_bal as i64,
            close: Some(close),
        };
    }

    pub fn reconstruct_signature_bls12(sig: &Vec<u8>) -> BoltResult<cl::Signature<Bls12>> {
        if (sig.len() != BLS12_381_G1_LEN * 2) {
            return Err(String::from("signature has invalid length"));
        }

        let mut cur_index = 0;
        let mut end_index = BLS12_381_G1_LEN;
        let ser_cl_h = sig[cur_index..end_index].to_vec();
        let str_cl_h = util::encode_as_hexstring(&ser_cl_h);
        let h = str_cl_h.as_bytes();

        cur_index = end_index;
        end_index += BLS12_381_G1_LEN;
        let ser_cl_H = sig[cur_index..end_index].to_vec();
        let str_cl_H = util::encode_as_hexstring(&ser_cl_H);
        let H = str_cl_H.as_bytes();

        let cl_sig = cl::Signature::<Bls12>::from_slice(&h, &H);

        Ok(Some(cl_sig))
    }

    pub fn reconstruct_channel_token_bls12(
        channel_token: &Vec<u8>,
    ) -> BoltResult<ChannelToken<Bls12>> {
        // parse pkc, pkm, pkM, mpk and comParams
        if channel_token.len() != BLS12_381_CHANNEL_TOKEN_LEN {
            return Err(String::from("could not reconstruct the channel token!"));
        }

        let num_y_elems = 5;
        let num_com_params = 6;

        let mut cur_index = 0;
        let mut end_index = SECP256K1_PK_LEN;
        let pkc = secp256k1::PublicKey::from_slice(&channel_token[cur_index..end_index]).unwrap();

        cur_index = end_index;
        end_index += SECP256K1_PK_LEN;
        let pkm = secp256k1::PublicKey::from_slice(&channel_token[cur_index..end_index]).unwrap();

        cur_index = end_index;
        end_index += BLS12_381_G2_LEN; // pk_M => (X, Y)
        let ser_cl_x = channel_token[cur_index..end_index].to_vec();
        let str_cl_x = util::encode_as_hexstring(&ser_cl_x);
        let X = str_cl_x.as_bytes();

        let mut Y = Vec::new();
        for _ in 0..num_y_elems {
            cur_index = end_index;
            end_index += BLS12_381_G2_LEN;
            let cl_y = channel_token[cur_index..end_index].to_vec();
            let ser_cl_y = util::encode_as_hexstring(&cl_y);
            let str_cl_y = ser_cl_y.as_bytes();
            Y.extend(str_cl_y);
        }
        let cl_pk =
            cl::PublicKey::<Bls12>::from_slice(&X, &Y.as_slice(), str_cl_x.len(), num_y_elems);

        cur_index = end_index;
        end_index += BLS12_381_G1_LEN;
        let g1 = channel_token[cur_index..end_index].to_vec();
        let ser_mpk_g1 = util::encode_as_hexstring(&g1);

        cur_index = end_index;
        end_index += BLS12_381_G2_LEN;
        let g2 = channel_token[cur_index..end_index].to_vec();
        let ser_mpk_g2 = util::encode_as_hexstring(&g2);

        let ser_g1 = ser_mpk_g1.as_bytes();
        let ser_g2 = ser_mpk_g2.as_bytes();

        let mpk = cl::PublicParams::<Bls12>::from_slice(&ser_g1, &ser_g2);

        let mut comparams = Vec::new();
        for _ in 0..num_com_params {
            cur_index = end_index;
            end_index += BLS12_381_G1_LEN;
            let com = channel_token[cur_index..end_index].to_vec();
            let ser_com = util::encode_as_hexstring(&com);
            let str_com = ser_com.as_bytes();
            comparams.extend(str_com);
        }

        let com_params = CSMultiParams::<Bls12>::from_slice(
            &comparams.as_slice(),
            ser_mpk_g1.len(),
            num_com_params,
        );

        Ok(Some(ChannelToken {
            pk_c: Some(pkc),
            pk_m: pkm,
            cl_pk_m: cl_pk,
            mpk: mpk,
            comParams: com_params,
        }))
    }

    ///
    /// Used in open-channel WTP for validating that a close_token is a valid signature
    ///
    pub fn tze_verify_cust_close_message(
        channel_token: &ChannelToken<Bls12>,
        wpk: &secp256k1::PublicKey,
        close_msg: &Wallet<Bls12>,
        close_token: &cl::Signature<Bls12>,
    ) -> bool {
        // close_msg => <pkc> || <wpk> || <balance-cust> || <balance-merch> || CLOSE
        // close_token = regular CL signature on close_msg
        // channel_token => <pk_c, CL_PK_m, pk_m, mpk, comParams>

        // (1) check that channel token and close msg are consistent (e.g., close_msg.channelId == H(channel_token.pk_c) &&
        let chan_token_cid = channel_token.compute_channel_id(); // util::hash_pubkey_to_fr::<Bls12>(&pk_c);
        let chan_token_wpk = util::hash_pubkey_to_fr::<Bls12>(&wpk);

        let cid_thesame = (close_msg.channelId == chan_token_cid);
        // (2) check that wpk matches what's in the close msg
        let wpk_thesame = (close_msg.wpk == chan_token_wpk);
        return cid_thesame
            && wpk_thesame
            && channel_token.cl_pk_m.verify(
                &channel_token.mpk,
                &close_msg.as_fr_vec(),
                &close_token,
            );
    }

    pub fn tze_generate_secp_signature(seckey: &[u8; 32], msg: &[u8; 32]) -> Vec<u8> {
        let secp = secp256k1::Secp256k1::signing_only();

        let msg = secp256k1::Message::from_slice(msg).unwrap();
        let seckey = secp256k1::SecretKey::from_slice(seckey).unwrap();
        let sig = secp.sign(&msg, &seckey);

        // get serialized signature
        let ser_sig = sig.serialize_der();

        return ser_sig.to_vec();
    }

    pub fn tze_verify_secp_signature(
        pubkey: &secp256k1::PublicKey,
        hash: &Vec<u8>,
        sig: &secp256k1::Signature,
    ) -> bool {
        let secp = secp256k1::Secp256k1::verification_only();
        let msg = secp256k1::Message::from_slice(hash.as_slice()).unwrap();

        return secp.verify(&msg, &sig, &pubkey).is_ok();
    }

    pub fn reconstruct_secp_channel_close_m(
        address: &[u8; ADDRESS_LEN],
        ser_revoke_token: &Vec<u8>,
        ser_sig: &Vec<u8>,
    ) -> ChannelcloseM {
        let revoke_token = secp256k1::Signature::from_der(&ser_revoke_token.as_slice()).unwrap();
        let sig = secp256k1::Signature::from_der(&ser_sig.as_slice()).unwrap();
        ChannelcloseM {
            address: hex::encode(&address.to_vec()),
            revoke: Some(revoke_token),
            signature: sig,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FundingTxInfo {
    pub init_cust_bal: i64,
    pub init_merch_bal: i64,
    pub escrow_txid: FixedSizeArray32,
    pub escrow_prevout: FixedSizeArray32,
    pub merch_txid: FixedSizeArray32,
    pub merch_prevout: FixedSizeArray32,
}

pub mod mpc {
    use bindings::{cb_receive, cb_send, ConnType_LNDNETIO, ConnType_NETIO};
    pub use channels_mpc::{
        ChannelMPCState, ChannelMPCToken, CustomerMPCState, MerchantMPCState, RevokedState,
        TransactionFeeInfo,
    };
    pub use channels_mpc::{
        ChannelStatus, InitCustState, NetworkConfig, PaymentStatus, ProtocolStatus,
    };
    use database::{MaskedTxMPCInputs, StateDatabase};
    use libc::c_void;
    use rand::Rng;
    use secp256k1::PublicKey;
    pub use wallet::{State, NONCE_LEN};
    use zkchan_tx::fixed_size_array::{FixedSizeArray16, FixedSizeArray32};
    use zkchan_tx::Testnet;

    ///
    /// init_merchant() - takes as input the public params, merchant balance and keypair.
    /// Generates merchant data which consists of channel token and merchant state.
    /// output: merchant state
    ///
    pub fn init_merchant<'a, R: Rng>(
        csprng: &mut R,
        db_url: String,
        channel_state: &mut ChannelMPCState,
        name: &'a str,
    ) -> MerchantMPCState {
        // create new merchant state
        let merch_name = String::from(name);
        let merch_state = MerchantMPCState::new(csprng, db_url, channel_state, merch_name);

        return merch_state;
    }

    ///
    /// init_customer() - takes as input the merchant's public key, and initial balance for customer and merchant.
    /// Generate initial customer channel state and channel token.
    /// output: a channel token and customer state
    ///
    pub fn init_customer<'a, R: Rng>(
        csprng: &mut R,
        pk_m: &PublicKey,
        b0_cust: i64,
        b0_merch: i64,
        tx_fee_info: &TransactionFeeInfo,
        name: &str,
    ) -> (ChannelMPCToken, CustomerMPCState) {
        assert!(b0_cust > 0);
        assert!(b0_merch >= 0);
        let bal_min_cust = tx_fee_info.bal_min_cust;
        let bal_min_merch = tx_fee_info.bal_min_merch;
        let val_cpfp = tx_fee_info.val_cpfp;
        let fee_cc = tx_fee_info.fee_cc;
        let fee_mc = tx_fee_info.fee_mc;
        let min_fee = tx_fee_info.min_fee;
        let max_fee = tx_fee_info.max_fee;

        let b0_cust = match b0_merch {
            0 => b0_cust - bal_min_cust - fee_mc - val_cpfp,
            _ => b0_cust,
        };

        let b0_merch = match b0_merch {
            0 => bal_min_merch + fee_mc + val_cpfp,
            _ => b0_merch,
        };

        let cust_name = String::from(name);
        let mut cust_state = CustomerMPCState::new(csprng, b0_cust, b0_merch, fee_cc, cust_name);

        // generate the initial channel token and initial state
        let channel_token = cust_state.generate_init_state(csprng, &pk_m, min_fee, max_fee, fee_mc);

        (channel_token, cust_state)
    }

    ///
    /// get_initial_state() - takes as input the customer state
    /// output: initial cust state and expected hash
    ///
    pub fn get_initial_state(
        cust_state: &CustomerMPCState,
    ) -> Result<(InitCustState, [u8; 32]), String> {
        let init_state = match cust_state.get_initial_cust_state() {
            Ok(n) => n,
            Err(e) => return Err(e.to_string()),
        };
        let init_state_hash = cust_state.get_current_state().compute_hash();
        Ok((init_state, init_state_hash))
    }

    ///
    /// validate_channel_params() - takes as input the channel token, initial state and verifies that they are well-formed
    /// output: true or false
    ///
    pub fn validate_channel_params(
        db: &mut dyn StateDatabase,
        channel_token: &ChannelMPCToken,
        init_state: &InitCustState,
        init_hash: [u8; 32],
        merch_state: &mut MerchantMPCState,
    ) -> Result<bool, String> {
        merch_state.validate_channel_params(db, channel_token, init_state, init_hash)
    }

    ///
    /// customer_mark_open_channel() - changes channel status in customer state
    ///
    pub fn customer_mark_open_channel(cust_state: &mut CustomerMPCState) -> Result<(), String> {
        cust_state.change_channel_status(ChannelStatus::Open)
    }

    ///
    /// merchant_mark_open_channel() - changes channel status for a given escrow-txid.
    /// fails if not in pending open state and assumes escrow-txid has been broadcast on chain
    ///
    pub fn merchant_mark_open_channel(
        escrow_txid_le: [u8; 32],
        merch_state: &mut MerchantMPCState,
    ) -> Result<(), String> {
        let mut escrow_txid_be = escrow_txid_le.clone();
        escrow_txid_be.reverse();
        merch_state.change_channel_status(escrow_txid_be, ChannelStatus::Open)
    }

    ///
    /// activate_customer() - takes as input an rng and the customer state.
    /// Prepare to activate the channel for the customer (call activate_customer_finalize to finalize activation)
    /// output: initial state
    ///
    pub fn activate_customer<R: Rng>(
        csprng: &mut R,
        cust_state: &mut CustomerMPCState,
    ) -> Result<State, String> {
        // check that customer already in the Initialized state
        if cust_state.protocol_status != ProtocolStatus::Initialized {
            return Err(format!(
                "invalid channel status for activate_customer(): {}",
                cust_state.protocol_status
            ));
        }

        let channel_status = cust_state.get_channel_status();
        if channel_status != ChannelStatus::Open {
            return Err(format!("channel is not open yet: {}", channel_status));
        }

        let _r_com = cust_state.generate_rev_lock_commitment(csprng);
        let _t0 = cust_state.get_randomness();

        Ok(cust_state.get_current_state())
    }

    ///
    /// activate_merchant() - takes as input a channel token, the intial state, and the merchant state.
    /// Activate the channel for the merchant
    /// output: intial pay token
    ///
    pub fn activate_merchant(
        db: &mut dyn StateDatabase,
        channel_token: ChannelMPCToken,
        s0: &State,
        merch_state: &mut MerchantMPCState,
    ) -> Result<[u8; 32], String> {
        // TODO: implement ZKC-19
        // activate channel - generate pay_token
        let mut escrow_txid_be = channel_token.escrow_txid.0.clone();
        escrow_txid_be.reverse();
        let channel_status = match merch_state.get_channel_status(escrow_txid_be) {
            Ok(s) => s,
            Err(e) => return Err(e.to_string()),
        };
        if channel_status != ChannelStatus::Open {
            return Err(format!("channel is not open yet: {}", channel_status));
        }
        merch_state.activate_channel(db, &channel_token, s0)
    }

    ///
    /// activate_customer_finalize() - takes as input the initial pay token and the customer state.
    /// Finalize activation of the channel for customer
    /// no output
    ///
    pub fn activate_customer_finalize(
        pay_token_0: [u8; 32],
        cust_state: &mut CustomerMPCState,
    ) -> Result<(), String> {
        cust_state.store_initial_pay_token(pay_token_0)
    }

    ///
    /// pay_prepare_customer() - takes as input an rng, the channel state, the payment amount, and the customer state.
    /// Prepare payment for customer
    /// output: new state (after payment), revocation lock commitment, revocation lock, revocation secret
    /// (only send revocation lock commitment to merchant)
    ///
    pub fn pay_prepare_customer<R: Rng>(
        csprng: &mut R,
        channel: &ChannelMPCState,
        amount: i64,
        cust_state: &mut CustomerMPCState,
    ) -> Result<(State, RevokedState, [u8; 32], [u8; 16]), String> {
        // verify that channel status is already activated or established
        if (cust_state.protocol_status == ProtocolStatus::Activated && amount >= 0)
            || (cust_state.protocol_status == ProtocolStatus::Established && amount > 0)
        {
            // check if payment on current balance is greater than dust limit
            let new_balance = match amount > 0 {
                true => cust_state.cust_balance - amount,  // positive value
                false => cust_state.cust_balance + amount, // negative value
            };
            if new_balance < channel.get_bal_min_cust() {
                let max_payment = cust_state.cust_balance - channel.get_bal_min_cust();
                let s = format!(
                    "Balance after payment is below dust limit: {}. Max payment: {}",
                    channel.get_bal_min_cust(),
                    max_payment
                );
                return Err(s);
            }
            let (cur_rev_lock, cur_rev_secret) = cust_state.get_rev_pair();
            // get current rev lock commitment
            let cur_rev_lock_com = cust_state.generate_rev_lock_commitment(csprng);
            // randomness for old rev lock commitment
            let cur_t = cust_state.get_randomness();

            cust_state.generate_new_state(csprng, amount);
            let new_state = cust_state.get_current_state();
            // pick new session ID
            let mut session_id = [0u8; 16];
            csprng.fill_bytes(&mut session_id);
            Ok((
                new_state,
                RevokedState {
                    rev_lock: FixedSizeArray32(cur_rev_lock),
                    rev_secret: FixedSizeArray32(cur_rev_secret),
                    t: FixedSizeArray16(cur_t),
                },
                cur_rev_lock_com,
                session_id,
            ))
        } else {
            return Err(format!(
                "Invalid channel status for pay_prepare_customer(): {}",
                cust_state.protocol_status
            ));
        }
    }

    ///
    /// pay_prepare_merchant() - takes as input an rng, the channel state, the nonce of the old state, rev lock commitment, amount and the merchant state.
    /// Prepare payment for merchant
    /// output: commitment of the payment token mask
    ///
    pub fn pay_prepare_merchant<R: Rng>(
        csprng: &mut R,
        db: &mut dyn StateDatabase,
        channel_state: &ChannelMPCState,
        session_id: [u8; 16],
        nonce: [u8; NONCE_LEN],
        rev_lock_com: [u8; 32],
        amount: i64,
        justification: Option<String>,
        merch_state: &mut MerchantMPCState,
    ) -> Result<[u8; 32], String> {
        // checks that no existing session with the specified session_id/nonce combo
        merch_state.generate_pay_mask_commitment(
            csprng,
            db,
            channel_state,
            session_id,
            nonce,
            rev_lock_com,
            amount,
            justification,
        )
    }

    ///
    /// pay_update_customer() - takes as input the channel state, the channel token, the intial state, the final state, a commitment for the mask for the pay token,
    /// the revocation lock commitment, the payment amount, and the customer state.
    /// Start the MPC for a payment for the Customer
    /// output: a success boolean, or error
    ///
    pub fn pay_update_customer(
        channel_state: &ChannelMPCState,
        channel_token: &ChannelMPCToken,
        s0: State,
        s1: State,
        pay_token_mask_com: [u8; 32],
        rev_lock_com: [u8; 32],
        amount: i64,
        cust_state: &mut CustomerMPCState,
        p_ptr: *mut c_void,
        send_cb: cb_send,
        receive_cb: cb_receive,
    ) -> Result<bool, String> {
        // verify that channel status is already activated or established (unlink)
        if (cust_state.protocol_status == ProtocolStatus::Activated && amount >= 0)
            || (cust_state.protocol_status == ProtocolStatus::Established && amount > 0)
        {
            cust_state.update_pay_com(pay_token_mask_com);
            if cust_state.net_config.is_none() {
                // use default
                let conn_type = match send_cb.is_some() && receive_cb.is_some() {
                    true => ConnType_LNDNETIO,
                    false => ConnType_NETIO,
                };
                cust_state.set_network_config(NetworkConfig {
                    conn_type,
                    dest_ip: String::from("127.0.0.1"),
                    dest_port: 2424,
                    path: String::new(),
                });
            }
            let circuit = cust_state.get_circuit_file();
            cust_state.execute_mpc_context(
                &channel_state,
                &channel_token,
                s0,
                s1,
                pay_token_mask_com,
                rev_lock_com,
                amount,
                circuit,
                p_ptr,
                send_cb,
                receive_cb,
            )
        } else {
            return Err(format!(
                "Invalid channel status for pay_update_customer(): {}",
                cust_state.protocol_status
            ));
        }
    }

    ///
    /// pay_update_merchant() - takes as input an rng, the channel state, the intial state, a commitment for the mask for the pay token,
    /// the revocation lock commitment, the payment amount, and the merchant state.
    /// Start the MPC for a payment for the Merchant
    /// output: the transaction masks (escrow and merch tx), or error
    ///
    pub fn pay_update_merchant<R: Rng>(
        csprng: &mut R,
        db: &mut dyn StateDatabase,
        channel: &ChannelMPCState,
        session_id: [u8; 16],
        pay_token_mask_com: [u8; 32],
        merch_state: &mut MerchantMPCState,
        p_ptr: *mut c_void,
        send_cb: cb_send,
        receive_cb: cb_receive,
    ) -> Result<bool, String> {
        if merch_state.net_config.is_none() {
            // use default ip/port
            let conn_type = match send_cb.is_some() && receive_cb.is_some() {
                true => ConnType_LNDNETIO,
                false => ConnType_NETIO,
            };
            merch_state.set_network_config(NetworkConfig {
                conn_type,
                dest_ip: String::from("127.0.0.1"),
                dest_port: 2424,
                path: String::new(),
            });
        }
        let circuit = merch_state.get_circuit_file();
        return merch_state.execute_mpc_context(
            csprng,
            db,
            &channel,
            session_id,
            pay_token_mask_com,
            circuit,
            p_ptr,
            send_cb,
            receive_cb,
        );
    }

    ///
    /// pay_confirm_mpc_result() - takes as input a db, session identifier, mpc result and merch state
    /// output: masked input if the mpc result was successful and there is a masked input for a given session_id
    ///
    pub fn pay_confirm_mpc_result(
        db: &mut dyn StateDatabase,
        session_id: [u8; 16],
        mpc_result: bool,
        _merch_state: &mut MerchantMPCState,
    ) -> Result<MaskedTxMPCInputs, String> {
        // check db is connected
        db.is_connected()?;

        let session_id_hex = hex::encode(session_id);
        match mpc_result {
            true => {
                let mask_bytes = match db.get_masked_mpc_inputs(&session_id_hex) {
                    Ok(n) => Some(n),
                    Err(e) => return Err(e.to_string()),
                };
                let mask_bytes_unwrapped = mask_bytes.unwrap();
                return Ok(mask_bytes_unwrapped.get_tx_masks());
            }
            false => {
                let mut session_state = match db.load_session_state(&session_id_hex) {
                    Ok(s) => s,
                    Err(e) => return Err(e.to_string()),
                };
                session_state.status = PaymentStatus::Error;
                db.update_session_state(&session_id_hex, &session_state);
                return Err(format!(
                    "pay_confirm_mpc_result: will need to restart MPC session"
                ));
            }
        }
    }

    ///
    /// pay_unmask_sigs_customer() - takes as input the transaction masks and the customer state.
    /// Unmask the transactions received from the MPC
    /// output: a success boolean
    ///
    pub fn pay_unmask_sigs_customer(
        channel_state: &ChannelMPCState,
        channel_token: &ChannelMPCToken,
        mask_bytes: MaskedTxMPCInputs,
        cust_state: &mut CustomerMPCState,
    ) -> Result<bool, String> {
        if (cust_state.protocol_status == ProtocolStatus::Activated
            || cust_state.protocol_status == ProtocolStatus::Established)
        {
            cust_state.unmask_and_verify_transactions::<Testnet>(
                channel_state,
                channel_token,
                mask_bytes,
            )
        } else {
            return Err(format!(
                "Invalid channel status for pay_unmask_sigs_customer(): {}",
                cust_state.protocol_status
            ));
        }
    }

    ///
    /// pay_validate_rev_lock_merchant() - takes as input the nonce, the revocation lock commitment, the revocation lock,
    /// the revocation secret and the merchant state.
    /// Verify the revocation lock commitment
    /// output: the pay token mask and randomness
    ///
    pub fn pay_validate_rev_lock_merchant(
        db: &mut dyn StateDatabase,
        session_id: [u8; 16],
        rev_state: RevokedState,
        merch_state: &mut MerchantMPCState,
    ) -> Result<([u8; 32], [u8; 16]), String> {
        let (pt_mask, pt_mask_r) = match merch_state.verify_revoked_state(
            db,
            session_id,
            rev_state.get_rev_lock(),
            rev_state.get_rev_secret(),
            rev_state.get_randomness(),
        ) {
            Ok(n) => (n.0, n.1),
            Err(e) => return Err(e.to_string()),
        };
        Ok((pt_mask, pt_mask_r))
    }

    ///
    /// pay_unmask_pay_token_customer() - takes as input the paytoken mask and the customer state.
    /// Verify the paytoken mask commitment and unmask paytoken
    /// output: success boolean
    ///
    pub fn pay_unmask_pay_token_customer(
        pt_mask_bytes: [u8; 32],
        pt_mask_r: [u8; 16],
        cust_state: &mut CustomerMPCState,
    ) -> Result<bool, String> {
        if (cust_state.protocol_status == ProtocolStatus::Activated
            || cust_state.protocol_status == ProtocolStatus::Established)
        {
            Ok(cust_state.unmask_and_verify_pay_token(pt_mask_bytes, pt_mask_r))
        } else {
            return Err(format!(
                "Invalid channel status for pay_unmask_pay_token_customer(): {}",
                cust_state.protocol_status
            ));
        }
    }

    ///
    /// force_customer_close() - takes as input the channel_state, channel_token, from_escrow and customer state.
    /// signs the closing tx on the current state of the channel
    /// output: cust-close-(signed_tx, txid) from escrow-tx or merch-close-tx
    ///
    pub fn force_customer_close(
        channel_state: &ChannelMPCState,
        channel_token: &ChannelMPCToken,
        from_escrow: bool,
        cust_state: &mut CustomerMPCState,
    ) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), String> {
        // (close_tx, close_txid_be, close_txid_le) that spends from escrow (if from_escrow = true)
        cust_state.customer_close::<Testnet>(&channel_state, &channel_token, from_escrow)
    }

    ///
    /// force_merchant_close() - takes as input the escrow txid and merchant state.
    /// signs the merch-close-tx tx on the current state of the channel
    /// output: merch-close-signed-tx on a given channel (identified by the escrow-txid)
    ///
    pub fn force_merchant_close(
        escrow_txid: &Vec<u8>,
        val_cpfp: i64,
        merch_state: &mut MerchantMPCState,
    ) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), String> {
        if escrow_txid.len() != 32 {
            return Err(format!(
                "escrow-txid does not have expected length: {}",
                escrow_txid.len()
            ));
        }
        let mut txid = [0u8; 32];
        txid.copy_from_slice(escrow_txid.as_slice());
        merch_state.get_closing_tx::<Testnet>(txid, val_cpfp)
    }
}

#[cfg(all(test, feature = "unstable"))]
mod benches {
    use rand::{thread_rng, Rng};
    use test::Bencher;

    #[bench]
    pub fn bench_one(bh: &mut Bencher) {
        println!("Run benchmark tests here!");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pairing::bls12_381::Bls12;
    use rand::{Rng, SeedableRng};
    use rand_xorshift::XorShiftRng;
    use sha2::{Digest, Sha256};

    use bindings::ConnType_NETIO;
    use channels_mpc::{ChannelStatus, PaymentStatus, ProtocolStatus};
    use database::{
        get_file_from_db, store_file_in_db, HashMapDatabase, MaskedTxMPCInputs, RedisDatabase,
        StateDatabase,
    };
    use std::process::Command;
    use std::{env, ptr};
    use zkchan_tx::fixed_size_array::FixedSizeArray32;
    use zkchan_tx::Testnet;

    fn setup_new_channel_helper(
        channel_state: &mut zkproofs::ChannelState<Bls12>,
        init_cust_bal: i64,
        init_merch_bal: i64,
    ) -> (
        zkproofs::ChannelToken<Bls12>,
        zkproofs::MerchantState<Bls12>,
        zkproofs::CustomerState<Bls12>,
        zkproofs::ChannelState<Bls12>,
    ) {
        let rng = &mut rand::thread_rng();
        let merch_name = "Bob";
        let cust_name = "Alice";

        let b0_cust = init_cust_bal;
        let b0_merch = init_merch_bal;

        // each party executes the init algorithm on the agreed initial challenge balance
        // in order to derive the channel tokens
        // initialize on the merchant side with balance: b0_merch
        let (mut channel_token, merch_state, channel_state) =
            zkproofs::init_merchant(rng, channel_state, merch_name);

        // initialize on the customer side with balance: b0_cust
        let cust_state =
            zkproofs::init_customer(rng, &mut channel_token, b0_cust, b0_merch, cust_name);

        return (channel_token, merch_state, cust_state, channel_state);
    }

    fn execute_establish_protocol_helper(
        channel_state: &mut zkproofs::ChannelState<Bls12>,
        channel_token: &mut zkproofs::ChannelToken<Bls12>,
        cust_balance: i64,
        merch_balance: i64,
        merch_state: &mut zkproofs::MerchantState<Bls12>,
        cust_state: &mut zkproofs::CustomerState<Bls12>,
    ) {
        let rng = &mut rand::thread_rng();

        // lets establish the channel
        let (com, com_proof) =
            zkproofs::establish_customer_generate_proof(rng, channel_token, cust_state);

        // obtain close token for closing out channel
        //let pk_h = hash_pubkey_to_fr::<Bls12>(&cust_state.pk_c.clone());
        let option = zkproofs::establish_merchant_issue_close_token(
            rng,
            &channel_state,
            &com,
            &com_proof,
            &cust_state.get_wallet().channelId,
            cust_balance,
            merch_balance,
            &merch_state,
        );
        let close_token = match option {
            Ok(n) => n.unwrap(),
            Err(e) => panic!(
                "Failed - zkproofs::establish_merchant_issue_close_token(): {}",
                e
            ),
        };
        assert!(cust_state.verify_close_token(&channel_state, &close_token));

        // wait for funding tx to be confirmed, etc

        // obtain payment token for pay protocol
        let pay_token =
            zkproofs::establish_merchant_issue_pay_token(rng, &channel_state, &com, &merch_state);
        //assert!(cust_state.verify_pay_token(&channel_state, &pay_token));

        assert!(zkproofs::establish_customer_final(
            channel_state,
            cust_state,
            &pay_token
        ));
        println!("Channel established!");
    }

    fn execute_payment_protocol_helper(
        channel_state: &mut zkproofs::ChannelState<Bls12>,
        merch_state: &mut zkproofs::MerchantState<Bls12>,
        cust_state: &mut zkproofs::CustomerState<Bls12>,
        payment_increment: i64,
    ) {
        let rng = &mut rand::thread_rng();

        let (payment, new_cust_state) =
            zkproofs::generate_payment_proof(rng, channel_state, &cust_state, payment_increment);

        let new_close_token =
            zkproofs::verify_payment_proof(rng, &channel_state, &payment, merch_state);

        let revoke_token = zkproofs::generate_revoke_token(
            &channel_state,
            cust_state,
            new_cust_state,
            &new_close_token,
        );

        // send revoke token and get pay-token in response
        let new_pay_token_result: BoltResult<cl::Signature<Bls12>> =
            zkproofs::verify_revoke_token(&revoke_token, merch_state);
        let new_pay_token = handle_bolt_result!(new_pay_token_result);

        // verify the pay token and update internal state
        assert!(cust_state.verify_pay_token(&channel_state, &new_pay_token.unwrap()));
    }

    #[test]
    fn bidirectional_payment_basics_work() {
        // just bidirectional case (w/o third party)
        let mut channel_state =
            zkproofs::ChannelState::<Bls12>::new(String::from("Channel A -> B"), false);
        let rng = &mut rand::thread_rng();

        let b0_customer = 90;
        let b0_merchant = 20;

        let (mut channel_token, mut merch_state, mut channel_state) =
            zkproofs::init_merchant(rng, &mut channel_state, "Merchant Bob");

        let mut cust_state =
            zkproofs::init_customer(rng, &mut channel_token, b0_customer, b0_merchant, "Alice");

        println!("{}", cust_state);

        // lets establish the channel
        let (com, com_proof) =
            zkproofs::establish_customer_generate_proof(rng, &mut channel_token, &mut cust_state);

        // obtain close token for closing out channel
        let option = zkproofs::establish_merchant_issue_close_token(
            rng,
            &channel_state,
            &com,
            &com_proof,
            &cust_state.get_wallet().channelId,
            b0_customer,
            b0_merchant,
            &merch_state,
        );
        let close_token = match option {
            Ok(n) => n.unwrap(),
            Err(e) => panic!(
                "Failed - zkproofs::establish_merchant_issue_close_token(): {}",
                e
            ),
        };
        assert!(cust_state.verify_close_token(&channel_state, &close_token));

        // wait for funding tx to be confirmed, etc

        // obtain payment token for pay protocol
        let pay_token =
            zkproofs::establish_merchant_issue_pay_token(rng, &channel_state, &com, &merch_state);
        //assert!(cust_state.verify_pay_token(&channel_state, &pay_token));

        assert!(zkproofs::establish_customer_final(
            &mut channel_state,
            &mut cust_state,
            &pay_token
        ));
        println!("Channel established!");

        let (payment, new_cust_state) =
            zkproofs::generate_payment_proof(rng, &channel_state, &cust_state, 10);

        let new_close_token =
            zkproofs::verify_payment_proof(rng, &channel_state, &payment, &mut merch_state);

        let revoke_token = zkproofs::generate_revoke_token(
            &channel_state,
            &mut cust_state,
            new_cust_state,
            &new_close_token,
        );

        // send revoke token and get pay-token in response
        let new_pay_token_result: BoltResult<cl::Signature<Bls12>> =
            zkproofs::verify_revoke_token(&revoke_token, &mut merch_state);
        let new_pay_token = handle_bolt_result!(new_pay_token_result);

        // verify the pay token and update internal state
        assert!(cust_state.verify_pay_token(&channel_state, &new_pay_token.unwrap()));

        println!("Successful payment!");

        let cust_close = zkproofs::customer_close(&channel_state, &cust_state);
        println!("Obtained the channel close message");
        println!("{}", cust_close.message);
        println!("close_token => {}", cust_close.merch_signature);
        println!("cust_sig => {}", cust_close.cust_signature);
    }

    #[test]
    fn bidirectional_multiple_payments_work() {
        let total_owed = 40;
        let b0_customer = 380;
        let b0_merchant = 20;
        let payment_increment = 20;

        let mut channel_state =
            zkproofs::ChannelState::<Bls12>::new(String::from("Channel A -> B"), false);

        // set fee for channel
        let fee = 5;
        channel_state.set_channel_fee(fee);

        let (mut channel_token, mut merch_state, mut cust_state, mut channel_state) =
            setup_new_channel_helper(&mut channel_state, b0_customer, b0_merchant);

        // run establish protocol for customer and merchant channel
        execute_establish_protocol_helper(
            &mut channel_state,
            &mut channel_token,
            b0_customer,
            b0_merchant,
            &mut merch_state,
            &mut cust_state,
        );

        assert!(channel_state.channel_established);

        {
            // make multiple payments in a loop
            let num_payments = total_owed / payment_increment;
            for _i in 0..num_payments {
                execute_payment_protocol_helper(
                    &mut channel_state,
                    &mut merch_state,
                    &mut cust_state,
                    payment_increment,
                );
            }

            {
                // scope localizes the immutable borrow here (for debug purposes only)
                println!("Customer balance: {:?}", &cust_state.cust_balance);
                println!("Merchant balance: {:?}", &cust_state.merch_balance);
                let total_owed_with_fees = (fee * num_payments) + total_owed;
                assert!(
                    cust_state.cust_balance == (b0_customer - total_owed_with_fees)
                        && cust_state.merch_balance == total_owed_with_fees + b0_merchant
                );
            }

            let cust_close_msg = zkproofs::customer_close(&channel_state, &cust_state);
            println!("Obtained the channel close message");
            println!("{}", cust_close_msg.message);
            println!("{}", cust_close_msg.merch_signature);
        }
    }

    #[test]
    fn bidirectional_payment_negative_payment_works() {
        // just bidirectional case (w/o third party)
        let total_owed = -20;
        let b0_customer = 90;
        let b0_merchant = 30;
        let payment_increment = -20;

        let mut channel_state =
            zkproofs::ChannelState::<Bls12>::new(String::from("Channel A -> B"), false);

        let (mut channel_token, mut merch_state, mut cust_state, mut channel_state) =
            setup_new_channel_helper(&mut channel_state, b0_customer, b0_merchant);

        // run establish protocol for customer and merchant channel
        execute_establish_protocol_helper(
            &mut channel_state,
            &mut channel_token,
            b0_customer,
            b0_merchant,
            &mut merch_state,
            &mut cust_state,
        );
        assert!(channel_state.channel_established);

        {
            execute_payment_protocol_helper(
                &mut channel_state,
                &mut merch_state,
                &mut cust_state,
                payment_increment,
            );

            {
                // scope localizes the immutable borrow here (for debug purposes only)
                println!("Customer balance: {:?}", &cust_state.cust_balance);
                println!("Merchant balance: {:?}", &cust_state.merch_balance);
                assert!(
                    cust_state.cust_balance == (b0_customer - total_owed)
                        && cust_state.merch_balance == total_owed + b0_merchant
                );
            }
        }
    }

    #[test]
    fn bidirectional_merchant_close_detects_double_spends() {
        let rng = &mut rand::thread_rng();

        let b0_customer = rng.gen_range(100, 1000);
        let b0_merchant = 10;
        let pay_increment = 20;

        let mut channel_state =
            zkproofs::ChannelState::<Bls12>::new(String::from("Channel A -> B"), false);

        let (mut channel_token, mut merch_state, mut cust_state, mut channel_state) =
            setup_new_channel_helper(&mut channel_state, b0_customer, b0_merchant);

        // run establish protocol for customer and merchant channel
        execute_establish_protocol_helper(
            &mut channel_state,
            &mut channel_token,
            b0_customer,
            b0_merchant,
            &mut merch_state,
            &mut cust_state,
        );

        assert!(channel_state.channel_established);

        // let's make a few payments then exit channel (will post an old channel state
        execute_payment_protocol_helper(
            &mut channel_state,
            &mut merch_state,
            &mut cust_state,
            pay_increment,
        );

        execute_payment_protocol_helper(
            &mut channel_state,
            &mut merch_state,
            &mut cust_state,
            pay_increment,
        );

        // let's close then move state forward
        let old_cust_close_msg = zkproofs::customer_close(&channel_state, &cust_state);

        execute_payment_protocol_helper(
            &mut channel_state,
            &mut merch_state,
            &mut cust_state,
            pay_increment,
        );

        execute_payment_protocol_helper(
            &mut channel_state,
            &mut merch_state,
            &mut cust_state,
            pay_increment,
        );
        let _cur_cust_close_msg = zkproofs::customer_close(&channel_state, &cust_state);

        let merch_close_result = zkproofs::merchant_close(
            &channel_state,
            &channel_token,
            &old_cust_close_msg,
            &merch_state,
        );
        let merch_close_msg = match merch_close_result {
            Ok(n) => n.unwrap(),
            Err(err) => panic!("Merchant close msg: {}", err),
        };

        println!("Double spend attempt by customer! Evidence below...");
        println!("Merchant close: wpk = {}", merch_close_msg.wpk);
        println!(
            "Merchant close: revoke_token = {}",
            merch_close_msg.revoke_token.unwrap()
        );
    }

    #[test]
    #[should_panic]
    fn bidirectional_merchant_close_works() {
        let rng = &mut rand::thread_rng();

        let b0_customer = rng.gen_range(100, 1000);
        let b0_merchant = 10;
        let pay_increment = 20;

        let mut channel_state =
            zkproofs::ChannelState::<Bls12>::new(String::from("Channel A -> B"), false);

        let (mut channel_token, mut merch_state, mut cust_state, mut channel_state) =
            setup_new_channel_helper(&mut channel_state, b0_customer, b0_merchant);

        // run establish protocol for customer and merchant channel
        execute_establish_protocol_helper(
            &mut channel_state,
            &mut channel_token,
            b0_customer,
            b0_merchant,
            &mut merch_state,
            &mut cust_state,
        );

        assert!(channel_state.channel_established);

        // let's make a few payments then exit channel (will post an old channel state
        execute_payment_protocol_helper(
            &mut channel_state,
            &mut merch_state,
            &mut cust_state,
            pay_increment,
        );

        execute_payment_protocol_helper(
            &mut channel_state,
            &mut merch_state,
            &mut cust_state,
            pay_increment,
        );

        execute_payment_protocol_helper(
            &mut channel_state,
            &mut merch_state,
            &mut cust_state,
            pay_increment,
        );

        execute_payment_protocol_helper(
            &mut channel_state,
            &mut merch_state,
            &mut cust_state,
            pay_increment,
        );

        let cust_close_msg = zkproofs::customer_close(&channel_state, &cust_state);

        let merch_close_result = zkproofs::merchant_close(
            &channel_state,
            &channel_token,
            &cust_close_msg,
            &merch_state,
        );
        let _merch_close_msg = match merch_close_result {
            Ok(n) => n.unwrap(),
            Err(err) => panic!("Merchant close msg: {}", err),
        };
    }

    #[test]
    fn intermediary_payment_basics_works() {
        println!("Intermediary test...");
        let rng = &mut rand::thread_rng();

        let b0_alice = rng.gen_range(100, 1000);
        let b0_bob = rng.gen_range(100, 1000);
        let b0_merch_a = rng.gen_range(100, 1000);
        let b0_merch_b = rng.gen_range(100, 1000);
        let tx_fee = rng.gen_range(1, 5);
        let mut channel_state =
            zkproofs::ChannelState::<Bls12>::new(String::from("New Channel State"), true);
        channel_state.set_channel_fee(tx_fee);

        let merch_name = "Hub";
        // each party executes the init algorithm on the agreed initial challenge balance
        // in order to derive the channel tokens
        // initialize on the merchant side with balance: b0_merch
        let (mut channel_token, mut merch_state, mut channel_state) =
            zkproofs::init_merchant(rng, &mut channel_state, merch_name);

        // initialize on the customer side with balance: b0_cust
        let mut alice_cust_state =
            zkproofs::init_customer(rng, &mut channel_token, b0_alice, b0_merch_a, "Alice");

        let mut bob_cust_state =
            zkproofs::init_customer(rng, &mut channel_token, b0_bob, b0_merch_b, "Bob");

        // run establish protocol for customer and merchant channel
        //let mut channel_state_alice = channel_state.clone();
        //let mut channel_state_bob = channel_state.clone();

        execute_establish_protocol_helper(
            &mut channel_state,
            &mut channel_token,
            b0_alice,
            b0_merch_a,
            &mut merch_state,
            &mut alice_cust_state,
        );
        execute_establish_protocol_helper(
            &mut channel_state,
            &mut channel_token,
            b0_bob,
            b0_merch_b,
            &mut merch_state,
            &mut bob_cust_state,
        );

        assert!(channel_state.channel_established);
        //assert!(channel_state_bob.channel_established);

        // run pay protocol - flow for third-party

        let amount = rng.gen_range(5, 100);
        let (sender_payment, new_alice_cust_state) =
            zkproofs::generate_payment_proof(rng, &channel_state, &alice_cust_state, amount);

        let (receiver_payment, new_bob_cust_state) =
            zkproofs::generate_payment_proof(rng, &channel_state, &bob_cust_state, -amount);

        // TODO: figure out how to attach conditions on payment recipients close token that they must (1) produce revocation token for sender's old wallet and (2) must have channel open

        // intermediary executes the following on the two payment proofs
        let close_token_result = zkproofs::verify_multiple_payment_proofs(
            rng,
            &channel_state,
            &sender_payment,
            &receiver_payment,
            &mut merch_state,
        );
        let (alice_close_token, bob_cond_close_token) =
            handle_bolt_result!(close_token_result).unwrap();

        // both alice and bob generate a revoke token
        let revoke_token_alice = zkproofs::generate_revoke_token(
            &channel_state,
            &mut alice_cust_state,
            new_alice_cust_state,
            &alice_close_token,
        );
        let revoke_token_bob = zkproofs::generate_revoke_token(
            &channel_state,
            &mut bob_cust_state,
            new_bob_cust_state,
            &bob_cond_close_token,
        );

        // send both revoke tokens to intermediary and get pay-tokens in response
        let new_pay_token_result: BoltResult<(cl::Signature<Bls12>, cl::Signature<Bls12>)> =
            zkproofs::verify_multiple_revoke_tokens(
                &revoke_token_alice,
                &revoke_token_bob,
                &mut merch_state,
            );
        let (new_pay_token_alice, new_pay_token_bob) =
            handle_bolt_result!(new_pay_token_result).unwrap();

        // verify the pay tokens and update internal state
        assert!(alice_cust_state.verify_pay_token(&channel_state, &new_pay_token_alice));
        assert!(bob_cust_state.verify_pay_token(&channel_state, &new_pay_token_bob));

        println!("Successful payment with intermediary!");
    }

    #[test]
    fn serialization_tests() {
        let mut channel_state =
            zkproofs::ChannelState::<Bls12>::new(String::from("Channel A -> B"), false);
        let rng = &mut rand::thread_rng();

        let serialized = serde_json::to_string(&channel_state).unwrap();
        println!("new channel state len: {}", &serialized.len());

        let _chan_state: zkproofs::ChannelState<Bls12> = serde_json::from_str(&serialized).unwrap();

        let (mut channel_token, _merch_state, _channel_state) =
            zkproofs::init_merchant(rng, &mut channel_state, "Merchant A");

        let b0_cust = 100;
        let b0_merch = 10;
        let cust_state =
            zkproofs::init_customer(rng, &mut channel_token, b0_cust, b0_merch, "Customer A");

        let serialized_ct = serde_json::to_string(&channel_token).unwrap();

        println!("serialized ct: {:?}", &serialized_ct);

        let _des_ct: zkproofs::ChannelToken<Bls12> = serde_json::from_str(&serialized_ct).unwrap();

        //println!("des_ct: {}", &des_ct);

        let serialized_cw = serde_json::to_string(&cust_state).unwrap();

        println!("serialized cw: {:?}", &serialized_cw);

        let _des_cw: zkproofs::CustomerState<Bls12> = serde_json::from_str(&serialized_cw).unwrap();
    }

    #[test]
    fn test_reconstruct_channel_token() {
        let _ser_channel_token = "024c252c7e36d0c30ae7c67dabea2168f41b36b85c14d3e180b423fa1a5df0e7ac027df0457901953b9b776f4999d5a1e78\
        049c0afa4f741d0d3bb7d9711a0f8c0038f4c70072363fe07ffe1450d63205cbaeaafe600ca9001d8bbf8984ce54a9c5e041084779dace7a4cf582906ea4e\
        493a1368ec7f05e7f89635c555c26e5d0149186095856dc210bef4b8fec03415cd6d1253bdafd0934a20b57ee088fa7ee0bab0668b1aa84c30e856dd685ce\
        e2a95844cb68504e82fd9dd874cbf6f7ee58155245e97c52625b53f4ca969f48b33c59f0009adc70d1472a303a35ace0d96149c8cdb96f29b6f476b8f4a10\
        bd430c4658d4e0b5873fcb946a76aa861c6c4c601ab8fb0b9c88d2e8861de2f0dae2bb2a8492db2978ce8f2e509328efbf12384ae2db5c17021d222724a3b\
        c4b621bf4f32601d555ff2cfc2171adeb2f1bd42c484c1c0a1e5d7d2853c102080680cefc925808b6e3d71b29a93f7e8f5c2eeeeef944b3740feddb24ec2c\
        17e3db22ee6a7af77e32a9d186bdcc150dd59b0cd92b92b6656cb588dec9d1d07be5e2a319bf37f1120b7c656f78dc6c4064f8d63f590f70cdc0c1746fde6\
        035eeb9aa90b69ea666ad71b27078ab61573aec60bab80a4e6a8e4d8ce02204f5b7e0131bf24d5df1428e9e571891c6feb1c0a52ba789136b244f13f510c4\
        f1f0eb4b0a7e675f105f8102c672461da340ebcae1eddd49a009bcf3b199eb2006fab6cf0ccf102b5c6dd45722dc0c27d4b9697f627f1bcbe44f6d96842de\
        c92877ff23d374964970c3386972a8ae369367907001bcd8bba458b8f29842321a8231f3441054999cb19b2c40409da8216406298e1d41bcaf5ea8a225266\
        2848d3f810dd369aba5ff684360080aa6f5e9ba61be1331f6bdf8b00d1ec8453637c4b480f6d0c5e5467013aa0e8be1777c370a1988db21d8d3de3f6d79d8\
        cbe6412f88d39de0cd1bf9e8f9b57ff933f21bef89b5bd3f9a901936568db58cc8326a719bf56438bbcab659a20ea5c0342eb9f072f105303c90de3b3b865\
        66155899d05d00396cfae74ac0526f0dd30c33e0c6790f3f8119dac12fb6f870b9a317afa94cd624b88ede30d49d2373b58453637c4b480f6d0c5e5467013\
        aa0e8be1777c370a1988db21d8d3de3f6d79d8cbe6412f88d39de0cd1bf9e8f9b57ffa397625c859a63e2c6e42486c4f76f306d484cce151f8614f87506e9\
        9c871521dd244bfeb380481aed8df823a507c7a3ad367c1797fc6efa089f929729e7d48bfa9c60860fbb212918bb91d8c6aa523046bdf208c95fa5a0fb86a\
        1e46f92e0e5893e136b74d38e106fa990590598932a4e2458034cea22337c6f365bcb5cab59ceea03d7a9f7821ea432e262877ef0128cb73d8733c3961762\
        26acb6b3de132c803be39a4e803cbc5a4670cb6169583fa899146fab0227dc2ae167393f96f3b8b31e015af1c305de3a07f52408e9c52495c2458ea05c7a3\
        71dc14f3b1d6a646ed7cc0ca9417d8bde6efc1ac300d8e28f";
        let ser_channel_token = hex::decode(_ser_channel_token).unwrap();

        let option_ct = tze_utils::reconstruct_channel_token_bls12(&ser_channel_token);
        let channel_token = match option_ct {
            Ok(n) => n.unwrap(),
            Err(e) => panic!("Error reconstructing compact rep of channel token: {}", e),
        };

        let channelId = channel_token.compute_channel_id();

        let original_channelId =
            "[\"e4f4bb9c5c64440788682c5ea06f457f265bd24186689fa50ce24a3be00c6107\"]";
        let computed_channelId = serde_json::to_string(&channelId).unwrap();

        println!("channel ID: {}", channelId);
        println!("pkc: {:?}", channel_token.pk_c.unwrap());
        println!("pkm: {:?}", channel_token.pk_m);

        assert_eq!(original_channelId, computed_channelId);

        // reconstruct signature
        let _ser_signature = "93f26490b4576c38dfb8dceae547f4b49aeb945ecc9cccc528c39068c78177bda68aaf45743f09c48ad99b6007fe415b\
                              aee9eafd51cfdb0dc567a5d152bc37861727e85088b417cf3ff57c108d0156eee56aff810f1e5f9e76cd6a3590d6db5e";
        let ser_signature = hex::decode(_ser_signature).unwrap();

        let option_sig = tze_utils::reconstruct_signature_bls12(&ser_signature);

        let _sig = match option_sig {
            Ok(n) => n.unwrap(),
            Err(e) => panic!("Error reconstructing compact rep of signature: {}", e),
        };
    }

    #[test]
    fn test_reconstruct_secp_sig() {
        let _ser_sig = "3044022064650285b55624f1f64b2c75e76589fa4b1033dabaa7ff50ff026e1dc038279202204ca696e0a829687c87171e8e5dab17069be248ff2595fd9607f3346dadcb579f";
        let ser_sig = hex::decode(_ser_sig).unwrap();

        let signature = tze_utils::reconstruct_secp_signature(ser_sig.as_slice());
        assert_eq!(format!("{:?}", signature), _ser_sig);

        let sk = hex::decode("81361b9bc2f67524dcc59b980dc8b06aadb77db54f6968d2af76ecdb612e07e4")
            .unwrap();
        let msg = "hello world!";
        let mut sha256 = sha2::Sha256::new();
        sha256.input(msg);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&sha256.result());

        let mut seckey = [0u8; 32];
        seckey.copy_from_slice(sk.as_slice());
        let sig = tze_utils::tze_generate_secp_signature(&seckey, &hash);
        assert!(sig.len() > 0);
    }

    #[test]
    fn test_reconstruct_channel_close_m() {
        let mut address = [0u8; 33];
        let address_slice =
            hex::decode("0a1111111111111111111111111111111111111111111111111111111111111111")
                .unwrap();
        address.copy_from_slice(address_slice.as_slice());

        let channelClose = tze_utils::reconstruct_secp_channel_close_m(&address,
                                                                       &hex::decode("3044022041932b376fe2c5e9e9ad0a3804e2290c3bc40617ea4f7b913be858dbcc3760b50220429d6eb1aabbd4135db4e0776c0b768af844b0af44f2f8f9da5a65e8541b4e9f").unwrap(),
                                                                       &hex::decode("3045022100e76653c5f8cb4c2f39efc7c5450d4f68ef3d84d482305534f5dfc310095a3124022003c4651ce1305cffe5e483ab99925cc4c9c5df2b5449bb18a51d52b21d789716").unwrap());

        assert_eq!(
            channelClose.address,
            "0a1111111111111111111111111111111111111111111111111111111111111111"
        );
        assert_eq!(format!("{:?}", channelClose.revoke.unwrap()), "3044022041932b376fe2c5e9e9ad0a3804e2290c3bc40617ea4f7b913be858dbcc3760b50220429d6eb1aabbd4135db4e0776c0b768af844b0af44f2f8f9da5a65e8541b4e9f");
        assert_eq!(format!("{:?}", channelClose.signature), "3045022100e76653c5f8cb4c2f39efc7c5450d4f68ef3d84d482305534f5dfc310095a3124022003c4651ce1305cffe5e483ab99925cc4c9c5df2b5449bb18a51d52b21d789716");
    }

    #[test]
    fn test_establish_mpc_channel() {
        let mut rng = &mut rand::thread_rng();
        // let mut db = RedisDatabase::new("lib", "redis://127.0.0.1/").unwrap();
        let mut db = HashMapDatabase::new("", "".to_string()).unwrap();

        let min_threshold = 546;
        let val_cpfp = 1000;
        let mut channel_state = mpc::ChannelMPCState::new(
            String::from("Channel A -> B"),
            1487,
            min_threshold,
            min_threshold,
            val_cpfp,
            false,
        );
        // init merchant
        let mut merch_state = mpc::init_merchant(rng, "".to_string(), &mut channel_state, "Bob");

        let fee_cc = 1000;
        let min_fee = 0;
        let max_fee = 10000;
        let fee_mc = 1000;
        let b0_cust = 10000;
        let b0_merch = 10000;

        let tx_fee_info = mpc::TransactionFeeInfo {
            bal_min_cust: min_threshold,
            bal_min_merch: min_threshold,
            val_cpfp: val_cpfp,
            fee_cc: fee_cc,
            fee_mc: fee_mc,
            min_fee: min_fee,
            max_fee: max_fee,
        };

        // init customer
        let (mut channel_token, mut cust_state) = mpc::init_customer(
            rng,
            &merch_state.pk_m,
            b0_cust,
            b0_merch,
            &tx_fee_info,
            "Alice",
        );

        // form all of the escrow and merch-close-tx transactions
        let funding_tx_info = generate_funding_tx(&mut rng, b0_cust, b0_merch);

        // form and sign the cust-close-from-escrow-tx and from-merch-close-tx
        let pubkeys = cust_state.get_pubkeys(&channel_state, &channel_token);

        // merchant signs the customer's closing transactions and sends signatures back to customer
        let to_self_delay_be = channel_state.get_self_delay_be(); // [0x05, 0xcf]; // big-endian format
        let (escrow_sig, merch_sig) = merch_state
            .sign_initial_closing_transaction::<Testnet>(
                funding_tx_info.clone(),
                pubkeys.rev_lock.0,
                pubkeys.cust_pk,
                pubkeys.cust_close_pk,
                to_self_delay_be,
                fee_cc,
                fee_mc,
                channel_state.get_val_cpfp(),
            )
            .unwrap();

        let res1 =
            cust_state.set_initial_cust_state(&mut channel_token, &funding_tx_info, &tx_fee_info);
        assert!(res1.is_ok(), res1.err().unwrap());

        let got_close_tx = cust_state.sign_initial_closing_transaction::<Testnet>(
            &channel_state,
            &channel_token,
            &escrow_sig,
            &merch_sig,
        );
        assert!(got_close_tx.is_ok(), got_close_tx.err().unwrap());
        // customer can proceed to sign the escrow-tx and merch-close-tx and sends resulting signatures to merchant
        let (init_cust_state, init_hash) = mpc::get_initial_state(&cust_state).unwrap();

        // at this point, the escrow-tx can be broadcast and confirmed
        let res2 = mpc::validate_channel_params(
            &mut db as &mut dyn StateDatabase,
            &channel_token,
            &init_cust_state,
            init_hash,
            &mut merch_state,
        );
        assert!(res2.is_ok(), res2.err().unwrap());
        let _rc = mpc::customer_mark_open_channel(&mut cust_state).unwrap();
        let _rc =
            mpc::merchant_mark_open_channel(channel_token.escrow_txid.0.clone(), &mut merch_state)
                .unwrap();

        let s0 = mpc::activate_customer(rng, &mut cust_state).unwrap();

        let pay_token = mpc::activate_merchant(
            &mut db as &mut dyn StateDatabase,
            channel_token,
            &s0,
            &mut merch_state,
        );
        assert!(pay_token.is_ok(), pay_token.err().unwrap());

        mpc::activate_customer_finalize(pay_token.unwrap(), &mut cust_state).unwrap();

        //TODO: test unlinking with a 0-payment of pay protocol
    }

    fn generate_funding_tx<R: Rng>(csprng: &mut R, b0_cust: i64, b0_merch: i64) -> FundingTxInfo {
        let mut escrow_txid = [0u8; 32];
        let mut merch_txid = [0u8; 32];

        csprng.fill_bytes(&mut escrow_txid);
        csprng.fill_bytes(&mut merch_txid);

        let mut escrow_prevout = [0u8; 32];
        let mut merch_prevout = [0u8; 32];

        let mut prevout_preimage1: Vec<u8> = Vec::new();
        prevout_preimage1.extend(escrow_txid.iter()); // txid1
        prevout_preimage1.extend(vec![0x00, 0x00, 0x00, 0x00]); // index
        let result1 = Sha256::digest(&Sha256::digest(&prevout_preimage1));
        escrow_prevout.copy_from_slice(&result1);

        let mut prevout_preimage2: Vec<u8> = Vec::new();
        prevout_preimage2.extend(merch_txid.iter()); // txid2
        prevout_preimage2.extend(vec![0x00, 0x00, 0x00, 0x00]); // index
        let result2 = Sha256::digest(&Sha256::digest(&prevout_preimage2));
        merch_prevout.copy_from_slice(&result2);

        return FundingTxInfo {
            init_cust_bal: b0_cust,
            init_merch_bal: b0_merch,
            escrow_txid: FixedSizeArray32(escrow_txid),
            merch_txid: FixedSizeArray32(merch_txid),
            escrow_prevout: FixedSizeArray32(escrow_prevout),
            merch_prevout: FixedSizeArray32(merch_prevout),
        };
    }

    fn setup_new_zkchannel_helper<R: Rng>(
        rng: &mut R,
        cust_bal: i64,
        merch_bal: i64,
        tx_fee_info: &mpc::TransactionFeeInfo,
    ) -> (
        mpc::ChannelMPCState,
        mpc::ChannelMPCToken,
        mpc::CustomerMPCState,
        mpc::MerchantMPCState,
    ) {
        // init channel state
        let mut channel_state = mpc::ChannelMPCState::new(
            String::from("Channel A -> B"),
            1487,
            tx_fee_info.bal_min_cust,
            tx_fee_info.bal_min_merch,
            tx_fee_info.val_cpfp,
            false,
        );
        // init merchant
        let merch_state = mpc::init_merchant(rng, "".to_string(), &mut channel_state, "Bob");

        let b0_cust = cust_bal;
        let b0_merch = merch_bal;
        // init customer
        let (channel_token, cust_state) = mpc::init_customer(
            rng,
            &merch_state.pk_m,
            b0_cust,
            b0_merch,
            tx_fee_info,
            "Alice",
        );

        return (channel_state, channel_token, cust_state, merch_state);
    }

    #[test]
    #[ignore]
    fn test_payment_mpc_channel_merch() {
        let mut rng = XorShiftRng::seed_from_u64(0x5dbe62598d313d76);
        let mut db = RedisDatabase::new("merch.lib", "redis://127.0.0.1/".to_string()).unwrap();

        let min_threshold = 546;
        let val_cpfp = 1000;
        let mut channel_state = mpc::ChannelMPCState::new(
            String::from("Channel A -> B"),
            1487,
            min_threshold,
            min_threshold,
            val_cpfp,
            false,
        );

        let mut merch_state =
            mpc::init_merchant(&mut rng, "".to_string(), &mut channel_state, "Bob");

        let b0_cust = 100000;
        let b0_merch = 100000;
        let fee_cc = 1000;
        let min_fee = 0;
        let max_fee = 10000;
        let fee_mc = 1000;
        let amount = 1000;
        let tx_fee_info = mpc::TransactionFeeInfo {
            bal_min_cust: min_threshold,
            bal_min_merch: min_threshold,
            val_cpfp: val_cpfp,
            fee_cc: fee_cc,
            fee_mc: fee_mc,
            min_fee: min_fee,
            max_fee: max_fee,
        };

        let (mut channel_token, mut cust_state) = mpc::init_customer(
            &mut rng,
            &merch_state.pk_m,
            b0_cust,
            b0_merch,
            &tx_fee_info,
            "Alice",
        );

        let funding_tx_info = generate_funding_tx(&mut rng, b0_cust, b0_merch);

        cust_state
            .set_initial_cust_state(&mut channel_token, &funding_tx_info, &tx_fee_info)
            .unwrap();

        let (init_cust_state, init_hash) = mpc::get_initial_state(&cust_state).unwrap();

        let res2 = mpc::validate_channel_params(
            &mut db as &mut dyn StateDatabase,
            &channel_token,
            &init_cust_state,
            init_hash,
            &mut merch_state,
        );
        println!("mpc::validate_channel_params: {}", res2.is_ok());

        // TODO: add cust-close tx signing API
        // transition state manually
        cust_state.protocol_status = ProtocolStatus::Initialized;
        let mut escrow_txid_be = channel_token.escrow_txid.0.clone();
        escrow_txid_be.reverse();
        let rc = cust_state.change_channel_status(ChannelStatus::PendingOpen);
        assert!(rc.is_ok());
        let rc = merch_state.change_channel_status(escrow_txid_be, ChannelStatus::PendingOpen);
        assert!(rc.is_ok());

        let _rc = mpc::customer_mark_open_channel(&mut cust_state).unwrap();
        let _rc =
            mpc::merchant_mark_open_channel(channel_token.escrow_txid.0.clone(), &mut merch_state)
                .unwrap();

        let s0 = mpc::activate_customer(&mut rng, &mut cust_state).unwrap();

        let pay_token = mpc::activate_merchant(
            &mut db as &mut dyn StateDatabase,
            channel_token.clone(),
            &s0,
            &mut merch_state,
        )
        .unwrap();

        mpc::activate_customer_finalize(pay_token, &mut cust_state).unwrap();

        let (_new_state, revoked_state, rev_lock_com, session_id) =
            mpc::pay_prepare_customer(&mut rng, &channel_state, amount, &mut cust_state).unwrap();

        let pay_mask_com = mpc::pay_prepare_merchant(
            &mut rng,
            &mut db as &mut dyn StateDatabase,
            &channel_state,
            session_id,
            s0.get_nonce(),
            rev_lock_com.clone(),
            amount,
            None,
            &mut merch_state,
        )
        .unwrap();

        let res_merch = mpc::pay_update_merchant(
            &mut rng,
            &mut db as &mut dyn StateDatabase,
            &channel_state,
            session_id,
            pay_mask_com,
            &mut merch_state,
            ptr::null_mut(),
            None,
            None,
        );
        assert!(res_merch.is_ok(), res_merch.err().unwrap());

        let mpc_result = res_merch.unwrap();
        let masked_inputs = mpc::pay_confirm_mpc_result(
            &mut db as &mut dyn StateDatabase,
            session_id.clone(),
            mpc_result,
            &mut merch_state,
        );
        assert!(masked_inputs.is_ok(), masked_inputs.err().unwrap());
        // println!("Masked Tx Inputs: {:#?}", masked_inputs.unwrap());
        let mask_in = masked_inputs.unwrap();
        println!("escrow_mask: {}", hex::encode(mask_in.escrow_mask.0));
        println!("merch_mask: {}", hex::encode(mask_in.merch_mask.0));
        println!("r_escrow_sig: {}", hex::encode(mask_in.r_escrow_sig.0));
        println!("r_merch_sig: {}", hex::encode(mask_in.r_merch_sig.0));

        let (pay_token_mask, pay_token_mask_r) = match mpc::pay_validate_rev_lock_merchant(
            &mut db as &mut dyn StateDatabase,
            session_id,
            revoked_state,
            &mut merch_state,
        ) {
            Ok(n) => (n.0, n.1),
            Err(e) => panic!("Could not get pay token mask and randomness: {}", e),
        };
        println!("pt_mask_r => {}", hex::encode(&pay_token_mask_r));
        assert_eq!(
            hex::encode(pay_token_mask),
            "6cd32e3254e7adaf3e742870ecab92aee1b863eabe75342a427d8e1954787822"
        );
        assert_eq!(
            hex::encode(pay_token_mask_r),
            "4a682bd5d46e3b5c7c6c353636086ed7"
        );
        // db.clear_state();
    }

    rusty_fork_test! {
        #[test]
        #[ignore]
        fn test_payment_mpc_channel_cust() {
            let mut rng = XorShiftRng::seed_from_u64(0x5dbe62598d313d76);
            let mut db = RedisDatabase::new("cust.lib", "redis://127.0.0.1/".to_string()).unwrap();

            let min_threshold = 546;
            let val_cpfp = 1000;
            let mut channel_state = mpc::ChannelMPCState::new(String::from("Channel A -> B"), 1487, min_threshold, min_threshold, val_cpfp, false);
            let mut merch_state = mpc::init_merchant(&mut rng, "".to_string(), &mut channel_state, "Bob");

            let b0_cust = 100000;
            let b0_merch = 100000;
            let fee_cc = 1000;
            let min_fee = 0;
            let max_fee = 10000;
            let fee_mc = 1000;
            let amount = 1000;
            let tx_fee_info = mpc::TransactionFeeInfo {
                bal_min_cust: min_threshold,
                bal_min_merch: min_threshold,
                val_cpfp: val_cpfp,
                fee_cc: fee_cc,
                fee_mc: fee_mc,
                min_fee: min_fee,
                max_fee: max_fee
            };

            let (mut channel_token, mut cust_state) = mpc::init_customer(&mut rng, &merch_state.pk_m, b0_cust, b0_merch, &tx_fee_info, "Alice");

            let funding_tx_info = generate_funding_tx(&mut rng, b0_cust, b0_merch);

            cust_state.set_initial_cust_state(&mut channel_token, &funding_tx_info, &tx_fee_info).unwrap();

            let (init_cust_state, init_hash) = match mpc::get_initial_state(&cust_state) {
                Ok(n) => (n.0, n.1),
                Err(e) => panic!(e)
            };

            let res2 = mpc::validate_channel_params(&mut db as &mut dyn StateDatabase, &channel_token, &init_cust_state, init_hash, &mut merch_state);
            println!("mpc::validate_channel_params: {}", res2.is_ok());

            // transition state manually
            cust_state.protocol_status = ProtocolStatus::Initialized;
            let mut escrow_txid_be = channel_token.escrow_txid.0.clone();
            escrow_txid_be.reverse();
            let rc = cust_state.change_channel_status(ChannelStatus::PendingOpen);
            assert!(rc.is_ok());
            let rc = merch_state.change_channel_status(escrow_txid_be, ChannelStatus::PendingOpen);
            assert!(rc.is_ok());

            let rc = mpc::customer_mark_open_channel(&mut cust_state);
            assert!(rc.is_ok());
            let rc = mpc::merchant_mark_open_channel(channel_token.escrow_txid.0.clone(), &mut merch_state);
            assert!(rc.is_ok());

            let s0 = mpc::activate_customer(&mut rng, &mut cust_state).unwrap();

            let pay_token = mpc::activate_merchant(&mut db as &mut dyn StateDatabase, channel_token.clone(), &s0, &mut merch_state).unwrap();

            mpc::activate_customer_finalize(pay_token, &mut cust_state).unwrap();

            let ser_tx_info = serde_json::to_string(&funding_tx_info).unwrap();
            println!("Ser Funding Tx Info: {}", ser_tx_info);
            let orig_funding_tx_info: FundingTxInfo = serde_json::from_str(&ser_tx_info).unwrap();
            assert_eq!(funding_tx_info, orig_funding_tx_info);

            let (state, _rev_state, rev_lock_com, session_id) = mpc::pay_prepare_customer(&mut rng, &mut channel_state, amount, &mut cust_state).unwrap();

            let pay_mask_com = mpc::pay_prepare_merchant(&mut rng, &mut db as &mut dyn StateDatabase, &channel_state, session_id, state.get_nonce(), rev_lock_com.clone(), amount, None, &mut merch_state).unwrap();

            let res_cust = mpc::pay_update_customer(&channel_state, &channel_token, s0, state, pay_mask_com, rev_lock_com, amount, &mut cust_state,
            ptr::null_mut(),
            None,
            None,);
            assert!(res_cust.is_ok() && res_cust.unwrap());

            let mut escrow_mask = [0u8; 32];
            escrow_mask.copy_from_slice(hex::decode("fddc371be95df8ea164916e88dcd895a1522fcff163fc3d70182c78d91d33699").unwrap().as_slice());
            let mut merch_mask = [0u8; 32];
            merch_mask.copy_from_slice(hex::decode("a943895982cb43deba0a8843459500e4671687f7cecc583745cd86342ddcccd4").unwrap().as_slice());
            let mut r_escrow_sig = [0u8; 32];
            r_escrow_sig.copy_from_slice(hex::decode("c1270ef7f78f7f8f208eb28da447d2e5820c9b7b9e37aee7f2f60af454d7ca31").unwrap().as_slice());
            let mut r_merch_sig = [0u8; 32];
            r_merch_sig.copy_from_slice(hex::decode("4bdedb34faa1b5374e86d5276cbb6fe31449252e3e959ff86a8506944d8d29d2").unwrap().as_slice());

            let masks = MaskedTxMPCInputs::new(
                escrow_mask,
                merch_mask,
                r_escrow_sig,
                r_merch_sig
            );

            let is_ok = mpc::pay_unmask_sigs_customer(&channel_state, &channel_token, masks, &mut cust_state);
            assert!(is_ok.is_ok(), is_ok.err().unwrap());

            let mut pt_mask = [0u8; 32];
            pt_mask.copy_from_slice(hex::decode("6cd32e3254e7adaf3e742870ecab92aee1b863eabe75342a427d8e1954787822").unwrap().as_slice());
            let mut pt_mask_r = [0u8; 16];
            pt_mask_r.copy_from_slice(hex::decode("4a682bd5d46e3b5c7c6c353636086ed7").unwrap().as_slice());

            let is_ok = mpc::pay_unmask_pay_token_customer(pt_mask, pt_mask_r, &mut cust_state).unwrap();
            assert!(is_ok);
        }
    }

    // establish the funding tx and sign initial closing tx
    fn establish_init_cust_close_tx_helper(
        funding_tx: &FundingTxInfo,
        tx_fee_info: &mpc::TransactionFeeInfo,
        channel_state: &mpc::ChannelMPCState,
        channel_token: &mut mpc::ChannelMPCToken,
        cust_state: &mut mpc::CustomerMPCState,
        merch_state: &mut mpc::MerchantMPCState,
    ) {
        cust_state
            .set_initial_cust_state(channel_token, funding_tx, tx_fee_info)
            .unwrap();
        let pubkeys = cust_state.get_pubkeys(&channel_state, &channel_token);

        let to_self_delay_be = channel_state.get_self_delay_be();
        // merchant signs and returns initial close sigs to customer
        let (escrow_sig, merch_sig) = merch_state
            .sign_initial_closing_transaction::<Testnet>(
                funding_tx.clone(),
                pubkeys.rev_lock.0,
                pubkeys.cust_pk,
                pubkeys.cust_close_pk,
                to_self_delay_be.clone(),
                tx_fee_info.fee_cc,
                tx_fee_info.fee_mc,
                tx_fee_info.val_cpfp,
            )
            .unwrap();

        assert!(cust_state.protocol_status == ProtocolStatus::New);

        // customer verifies the close signatures
        let got_close_tx = cust_state.sign_initial_closing_transaction::<Testnet>(
            &channel_state,
            &channel_token,
            &escrow_sig,
            &merch_sig,
        );
        assert!(got_close_tx.is_ok(), got_close_tx.err().unwrap());

        // at this point, we should be pending open since we've got the initial close tx signed
        // just need to broadcast the escrow tx
        assert!(cust_state.get_channel_status() == ChannelStatus::PendingOpen);
    }

    // establish the init merch-close-tx
    fn establish_merch_close_tx_helper(
        funding_tx_info: &mut FundingTxInfo,
        channel_state: &mpc::ChannelMPCState,
        channel_token: &mpc::ChannelMPCToken,
        cust_bal: i64,
        merch_bal: i64,
        cust_state: &mut mpc::CustomerMPCState,
        merch_state: &mut mpc::MerchantMPCState,
        fee_mc: i64,
    ) {
        let escrow_txid_be = funding_tx_info.escrow_txid.0.clone();
        let to_self_delay_be = channel_state.get_self_delay_be();
        let pubkeys = cust_state.get_pubkeys(&channel_state, &channel_token);
        let cust_sk = cust_state.get_close_secret_key();

        let (merch_tx_preimage, tx_params) =
            zkchan_tx::transactions::btc::merchant_form_close_transaction::<Testnet>(
                escrow_txid_be.to_vec(),
                pubkeys.cust_pk.clone(),
                pubkeys.merch_pk.clone(),
                pubkeys.merch_close_pk.clone(),
                cust_bal,
                merch_bal,
                fee_mc,
                channel_state.get_val_cpfp(),
                to_self_delay_be.clone(),
            )
            .unwrap();

        // set the funding_tx_info structure
        let (merch_txid_be, prevout) =
            zkchan_tx::txutil::merchant_generate_transaction_id(tx_params).unwrap();
        funding_tx_info.merch_txid = FixedSizeArray32(merch_txid_be);
        funding_tx_info.merch_prevout = FixedSizeArray32(prevout);

        // generate merch-close tx
        let cust_sig =
            zkchan_tx::txutil::customer_sign_merch_close_transaction(&cust_sk, &merch_tx_preimage)
                .unwrap();

        let _is_ok = zkchan_tx::txutil::merchant_verify_merch_close_transaction(
            &merch_tx_preimage,
            &cust_sig,
            &pubkeys.cust_pk,
        )
        .unwrap();

        // store the signature for merch-close-tx
        merch_state.store_merch_close_tx(
            &escrow_txid_be.to_vec(),
            &pubkeys.cust_pk,
            cust_bal,
            merch_bal,
            fee_mc,
            to_self_delay_be,
            &cust_sig,
        );
    }

    // validate the initial state of the channel
    fn validate_initial_channel_state_helper(
        db: &mut RedisDatabase,
        channel_token: &mpc::ChannelMPCToken,
        cust_state: &mut mpc::CustomerMPCState,
        merch_state: &mut mpc::MerchantMPCState,
    ) {
        let (init_state, init_hash) = mpc::get_initial_state(&cust_state).unwrap();

        assert!(mpc::validate_channel_params(
            db as &mut dyn StateDatabase,
            &channel_token,
            &init_state,
            init_hash,
            merch_state
        )
        .unwrap());
    }

    // run activate sub protocol between customer/merchant
    fn activate_channel_helper<R: Rng>(
        rng: &mut R,
        db: &mut RedisDatabase,
        channel_token: &mpc::ChannelMPCToken,
        cust_state: &mut mpc::CustomerMPCState,
        merch_state: &mut mpc::MerchantMPCState,
    ) {
        let s0_result = mpc::activate_customer(rng, cust_state);
        assert!(s0_result.is_ok());
        let s0 = s0_result.unwrap();

        let pay_token_result = mpc::activate_merchant(
            db as &mut dyn StateDatabase,
            channel_token.clone(),
            &s0,
            merch_state,
        );
        assert!(pay_token_result.is_ok());
        let pay_token = pay_token_result.unwrap();

        let res = mpc::activate_customer_finalize(pay_token, cust_state);
        assert!(res.is_ok());
    }

    // run pay prepare between customer and merchant
    fn pay_prepare_helper<R: Rng>(
        rng: &mut R,
        db: &mut RedisDatabase,
        channel_state: &mpc::ChannelMPCState,
        cust_state: &mut mpc::CustomerMPCState,
        amount: i64,
        merch_state: &mut mpc::MerchantMPCState,
    ) -> (
        [u8; 16],
        mpc::State,
        mpc::State,
        mpc::RevokedState,
        [u8; 32],
        [u8; 32],
    ) {
        // get the old state
        let cur_state = cust_state.get_current_state();
        // let's prepare a new payment
        let (new_state, rev_state, rev_lock_com, session_id) =
            mpc::pay_prepare_customer(rng, channel_state, amount, cust_state).unwrap();

        // println!("Old Nonce: {}", hex::encode(&cur_state.get_nonce()));
        let justification = match amount < 0 {
            true => Some(format!("empty-sig")),
            false => None,
        };
        let pay_mask_com = mpc::pay_prepare_merchant(
            rng,
            db as &mut dyn StateDatabase,
            channel_state,
            session_id,
            cur_state.get_nonce(),
            rev_lock_com.clone(),
            amount,
            justification,
            merch_state,
        )
        .unwrap();

        return (
            session_id,
            cur_state,
            new_state,
            rev_state,
            rev_lock_com,
            pay_mask_com,
        );
    }

    #[test]
    fn test_channel_activated_correctly() {
        let mut rng = XorShiftRng::seed_from_u64(0xc7175992415de87a);
        let mut db = RedisDatabase::new("mpclib", "redis://127.0.0.1/".to_string()).unwrap();
        db.clear_state();

        let b0_cust = 10000;
        let b0_merch = 10000;
        let fee_cc = 1000;
        let fee_mc = 1000;
        let min_fee = 0;
        let max_fee = 10000;
        let min_threshold = 546; // dust limit
        let val_cpfp = 1000;

        let tx_fee_info = mpc::TransactionFeeInfo {
            bal_min_cust: min_threshold,
            bal_min_merch: min_threshold,
            val_cpfp: val_cpfp,
            fee_cc: fee_cc,
            fee_mc: fee_mc,
            min_fee: min_fee,
            max_fee: max_fee,
        };

        let (channel_state, mut channel_token, mut cust_state, mut merch_state) =
            setup_new_zkchannel_helper(&mut rng, b0_cust, b0_merch, &tx_fee_info);

        // create funding txs
        let funding_tx_info = generate_funding_tx(&mut rng, b0_cust, b0_merch);

        // customer obtains signatures on initial closing tx
        establish_init_cust_close_tx_helper(
            &funding_tx_info,
            &tx_fee_info,
            &channel_state,
            &mut channel_token,
            &mut cust_state,
            &mut merch_state,
        );

        assert!(cust_state.protocol_status == ProtocolStatus::Initialized);

        // merchant validates the initial state
        validate_initial_channel_state_helper(
            &mut db,
            &channel_token,
            &mut cust_state,
            &mut merch_state,
        );
        println!("initial channel state validated!");
        // println!("cust_state channel status: {}", cust_state.protocol_status);

        let rc = mpc::customer_mark_open_channel(&mut cust_state);
        assert!(rc.is_ok());
        let rc =
            mpc::merchant_mark_open_channel(channel_token.escrow_txid.0.clone(), &mut merch_state);
        assert!(rc.is_ok());

        activate_channel_helper(
            &mut rng,
            &mut db,
            &channel_token,
            &mut cust_state,
            &mut merch_state,
        );
        assert!(cust_state.protocol_status == ProtocolStatus::Activated);
        println!("cust_state channel status: {}", cust_state.protocol_status);
    }

    fn zkchannel_full_establish_setup_helper<R: Rng>(
        rng: &mut R,
        db: &mut RedisDatabase,
        tx_fee_info: &mpc::TransactionFeeInfo,
    ) -> (
        mpc::ChannelMPCState,
        mpc::ChannelMPCToken,
        mpc::CustomerMPCState,
        mpc::MerchantMPCState,
    ) {
        let b0_cust = 10000;
        let b0_merch = 10000;

        let (channel_state, mut channel_token, mut cust_state, mut merch_state) =
            setup_new_zkchannel_helper(rng, b0_cust, b0_merch, &tx_fee_info);

        // generate random funding tx for testing
        let mut funding_tx_info = generate_funding_tx(rng, b0_cust, b0_merch);

        // customer and merchant jointly sign merch-close-tx
        establish_merch_close_tx_helper(
            &mut funding_tx_info,
            &channel_state,
            &channel_token,
            b0_cust,
            b0_merch,
            &mut cust_state,
            &mut merch_state,
            tx_fee_info.fee_mc,
        );

        // customer obtains signatures on initial closing tx
        establish_init_cust_close_tx_helper(
            &funding_tx_info,
            tx_fee_info,
            &channel_state,
            &mut channel_token,
            &mut cust_state,
            &mut merch_state,
        );
        assert!(cust_state.protocol_status == ProtocolStatus::Initialized);

        //println!("channel_token: {:?}", cust_state);

        // merchant validates the initial state
        validate_initial_channel_state_helper(
            db,
            &channel_token,
            &mut cust_state,
            &mut merch_state,
        );

        // if escrow-tx confirmed on chain, can proceed to change status for both customer/merchant
        let rc = mpc::customer_mark_open_channel(&mut cust_state);
        assert!(rc.is_ok());
        let rc =
            mpc::merchant_mark_open_channel(channel_token.escrow_txid.0.clone(), &mut merch_state);
        assert!(rc.is_ok());

        // customer/merchant activate the channel
        activate_channel_helper(rng, db, &channel_token, &mut cust_state, &mut merch_state);
        assert!(cust_state.protocol_status == ProtocolStatus::Activated);
        println!("cust_state channel status: {}", cust_state.protocol_status);

        return (channel_state, channel_token, cust_state, merch_state);
    }

    fn complete_pay_helper(
        merch_db: &mut RedisDatabase,
        session_id: [u8; 16],
        rev_state: mpc::RevokedState,
        channel_state: &mpc::ChannelMPCState,
        channel_token: &mpc::ChannelMPCToken,
        cust_state: &mut mpc::CustomerMPCState,
        merch_state: &mut mpc::MerchantMPCState,
    ) {
        let mask_bytes = mpc::pay_confirm_mpc_result(
            merch_db as &mut dyn StateDatabase,
            session_id.clone(),
            true,
            merch_state,
        )
        .unwrap();

        println!("complete_pay_helper - got the mask bytes: {:?}", mask_bytes);

        // unmask the closing tx
        let is_sigs_ok =
            mpc::pay_unmask_sigs_customer(&channel_state, &channel_token, mask_bytes, cust_state)
                .unwrap();
        assert!(is_sigs_ok);

        // merchant validates the old state
        let (pt_mask, pt_mask_r) = match mpc::pay_validate_rev_lock_merchant(
            merch_db as &mut dyn StateDatabase,
            session_id,
            rev_state,
            merch_state,
        ) {
            Ok(n) => (n.0, n.1),
            Err(e) => {
                println!("Could not get pay token mask and randomness: {}", e);
                return;
            }
        };

        println!(
            "complete_pay_helper - new pay token: {}",
            hex::encode(&pt_mask)
        );

        // unmask pay_token
        let is_ok = mpc::pay_unmask_pay_token_customer(pt_mask, pt_mask_r, cust_state).unwrap();
        assert!(is_ok);
    }

    fn load_merchant_state_info(
        db_conn: &mut redis::Connection,
        db_key: &String,
        merch_state_key: &String,
    ) -> Result<mpc::MerchantMPCState, String> {
        // load the merchant state from DB
        let ser_merch_state = get_file_from_db(db_conn, &db_key, &merch_state_key).unwrap();
        let merch_state: mpc::MerchantMPCState = serde_json::from_str(&ser_merch_state).unwrap();
        Ok(merch_state)
    }

    fn save_merchant_state_info(
        db_conn: &mut redis::Connection,
        db_key: &String,
        channel_state_key: &String,
        channel_state: Option<&mpc::ChannelMPCState>,
        merch_state_key: &String,
        merch_state: &mpc::MerchantMPCState,
    ) -> Result<(), String> {
        // let key = String::from("cli:merch_db");
        match channel_state {
            Some(n) => {
                let channel_state_json_str = serde_json::to_string(n).unwrap();
                store_file_in_db(
                    db_conn,
                    &db_key,
                    &channel_state_key,
                    &channel_state_json_str,
                )?
            }
            None => false, // do nothing
        };

        let merch_state_json_str = serde_json::to_string(merch_state).unwrap();
        store_file_in_db(db_conn, &db_key, &merch_state_key, &merch_state_json_str)?;
        Ok(())
    }

    fn run_mpctest_as_merchant(
        db: &mut RedisDatabase,
        db_key: &String,
        session_id: [u8; 16],
        pay_mask_com: [u8; 32],
        channel_state: &mpc::ChannelMPCState,
        merch_state_key: &String,
        merch_state: &mpc::MerchantMPCState,
    ) -> std::process::Child {
        let cur_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let mut profile = "release";
        if cfg!(debug_assertions) {
            profile = "debug";
        }
        let mpc_test_bin = format!("{}/target/{}/mpctest", cur_dir, profile);
        println!("mpctest path: {}", mpc_test_bin);

        let session_id_arg = format!("{}", hex::encode(session_id));
        let pay_mask_com_arg = format!("{}", hex::encode(pay_mask_com));

        // let's start a thread but block until we get to pay_update_customer()
        let channel_state_key = "channel_state".to_string();
        save_merchant_state_info(
            &mut db.conn,
            db_key,
            &channel_state_key,
            Some(&channel_state),
            &merch_state_key,
            &merch_state,
        )
        .unwrap();

        let child = Command::new(mpc_test_bin)
            .arg("--db-key")
            .arg(db_key.clone())
            .arg("--pay-mask-com")
            .arg(pay_mask_com_arg)
            .arg("--session-id")
            .arg(session_id_arg)
            .spawn()
            .expect("failed to execute mpctest");

        return child;
    }

    #[test]
    fn test_unlink_and_pay_is_correct() {
        let mut rng = &mut rand::thread_rng();
        let mut db = RedisDatabase::new("mpctest", "redis://127.0.0.1/".to_string()).unwrap();
        db.clear_state();

        // full channel setup
        let fee_cc = 1000;
        let fee_mc = 1000;
        let min_fee = 0;
        let max_fee = 10000;
        let min_threshold = 546; // dust limit
        let val_cpfp = 1000;

        let tx_fee_info = mpc::TransactionFeeInfo {
            bal_min_cust: min_threshold,
            bal_min_merch: min_threshold,
            val_cpfp: val_cpfp,
            fee_cc: fee_cc,
            fee_mc: fee_mc,
            min_fee: min_fee,
            max_fee: max_fee,
        };

        let (channel_state, channel_token, mut cust_state, mut merch_state) =
            zkchannel_full_establish_setup_helper(&mut rng, &mut db, &tx_fee_info);

        // UNLINK PROTOCOL
        let (session_id, cur_state, new_state, rev_state, rev_lock_com, pay_mask_com) =
            pay_prepare_helper(
                &mut rng,
                &mut db,
                &channel_state,
                &mut cust_state,
                0,
                &mut merch_state,
            );

        let nc = channels_mpc::NetworkConfig {
            conn_type: ConnType_NETIO,
            path: String::from("tmpsock"),
            dest_ip: String::from("127.0.0.1"),
            dest_port: 5000,
        };
        cust_state.set_network_config(nc.clone());
        merch_state.set_network_config(nc.clone());

        let db_key = "mpctest:merch_db".to_string();
        let merch_state_key = "merch_state".to_string();
        let mut mpc_child = run_mpctest_as_merchant(
            &mut db,
            &db_key,
            session_id.clone(),
            pay_mask_com,
            &channel_state,
            &merch_state_key,
            &merch_state,
        );

        // pay update for customer
        let res_cust = mpc::pay_update_customer(
            &channel_state,
            &channel_token,
            cur_state,
            new_state,
            pay_mask_com,
            rev_lock_com,
            0,
            &mut cust_state,
            ptr::null_mut(),
            None,
            None,
        );
        assert!(res_cust.is_ok());
        let mpc_result_ok = res_cust.unwrap();
        assert!(mpc_result_ok);

        // wait for mpctest to complete execution
        let ecode = mpc_child.wait().expect("failed to wait on mpctest");
        assert!(ecode.success());

        // load the updated merchant state
        let mut merch_state =
            load_merchant_state_info(&mut db.conn, &db_key, &merch_state_key).unwrap();

        // complete the rest of unlink
        complete_pay_helper(
            &mut db,
            session_id,
            rev_state,
            &channel_state,
            &channel_token,
            &mut cust_state,
            &mut merch_state,
        );

        println!("cust state: {:?}", cust_state.get_current_state());
        println!("customer's channel status: {}", cust_state.protocol_status);

        assert!(cust_state.protocol_status == ProtocolStatus::Established);

        // PAY PROTOCOL
        let (session_id1, cur_state1, new_state1, rev_state1, rev_lock_com1, pay_mask_com1) =
            pay_prepare_helper(
                &mut rng,
                &mut db,
                &channel_state,
                &mut cust_state,
                200,
                &mut merch_state,
            );

        let mut mpc_child = run_mpctest_as_merchant(
            &mut db,
            &db_key,
            session_id1.clone(),
            pay_mask_com1,
            &channel_state,
            &merch_state_key,
            &merch_state,
        );

        // pay update for customer
        let res_cust = mpc::pay_update_customer(
            &channel_state,
            &channel_token,
            cur_state1,
            new_state1,
            pay_mask_com1,
            rev_lock_com1,
            200,
            &mut cust_state,
            ptr::null_mut(),
            None,
            None,
        );
        assert!(res_cust.is_ok());
        let mpc_result_ok = res_cust.unwrap();
        assert!(mpc_result_ok);

        let ecode = mpc_child.wait().expect("failed to wait on mpctest");
        assert!(ecode.success());

        // load the updated merchant state
        let merch_state_key = "merch_state".to_string();
        let mut merch_state =
            load_merchant_state_info(&mut db.conn, &db_key, &merch_state_key).unwrap();

        // complete the rest of unlink
        complete_pay_helper(
            &mut db,
            session_id1,
            rev_state1,
            &channel_state,
            &channel_token,
            &mut cust_state,
            &mut merch_state,
        );

        let res = cust_state.change_channel_status(ChannelStatus::PendingClose);
        assert!(res.is_err());

        let res = cust_state.change_channel_status(ChannelStatus::ConfirmedClose);
        assert!(res.is_err());

        // customer initiates close tx
        let (_cust_close_signed_tx, _close_txid_be, _close_txid_le) =
            mpc::force_customer_close(&channel_state, &channel_token, true, &mut cust_state)
                .unwrap();

        assert_eq!(
            cust_state.get_channel_status(),
            ChannelStatus::CustomerInitClose
        );

        let mut escrow_txid_be = channel_token.escrow_txid.0.clone(); // originally in LE
        escrow_txid_be.reverse();
        let (_merch_close_signed_tx, _merch_txid_be, _merch_txid_le) = mpc::force_merchant_close(
            &escrow_txid_be.to_vec(),
            channel_state.get_val_cpfp(),
            &mut merch_state,
        )
        .unwrap();
        assert!(
            merch_state.get_channel_status(escrow_txid_be).unwrap()
                == ChannelStatus::MerchantInitClose
        );

        // change close status after closing transaction is detected on-chain
        let res = cust_state.change_channel_status(ChannelStatus::PendingClose);
        assert!(res.is_ok());
        assert_eq!(cust_state.get_channel_status(), ChannelStatus::PendingClose);

        // assume that timelock has passed and there was no dispute
        let res = cust_state.change_channel_status(ChannelStatus::ConfirmedClose);
        assert!(res.is_ok());
        assert_eq!(
            cust_state.get_channel_status(),
            ChannelStatus::ConfirmedClose
        );
    }

    #[test]
    fn test_unlink_fail_as_expected() {
        let mut rng = &mut rand::thread_rng();
        let mut db = RedisDatabase::new("mpctest", "redis://127.0.0.1/".to_string()).unwrap();
        db.clear_state();

        // full channel setup
        let fee_cc = 1000;
        let fee_mc = 1000;
        let min_fee = 0;
        let max_fee = 10000;
        let min_threshold = 546; // dust limit
        let val_cpfp = 1000;
        let tx_fee_info = mpc::TransactionFeeInfo {
            bal_min_cust: min_threshold,
            bal_min_merch: min_threshold,
            val_cpfp: val_cpfp,
            fee_cc: fee_cc,
            fee_mc: fee_mc,
            min_fee: min_fee,
            max_fee: max_fee,
        };

        let (channel_state, channel_token, mut cust_state, mut merch_state) =
            zkchannel_full_establish_setup_helper(&mut rng, &mut db, &tx_fee_info);

        // UNLINK PROTOCOL
        let (session_id, cur_state, new_state, _rev_state, rev_lock_com, pay_mask_com) =
            pay_prepare_helper(
                &mut rng,
                &mut db,
                &channel_state,
                &mut cust_state,
                0,
                &mut merch_state,
            );

        let nc = channels_mpc::NetworkConfig {
            conn_type: ConnType_NETIO,
            path: String::from("tmpsock"),
            dest_ip: String::from("127.0.0.1"),
            dest_port: 5000,
        };
        cust_state.set_network_config(nc.clone());
        merch_state.set_network_config(nc.clone());

        let db_key = "mpctest:merch_db".to_string();
        let merch_state_key = "merch_state".to_string();
        let mut mpc_child = run_mpctest_as_merchant(
            &mut db,
            &db_key,
            session_id,
            pay_mask_com,
            &channel_state,
            &merch_state_key,
            &merch_state,
        );

        // pay update for customer
        let res_cust = mpc::pay_update_customer(
            &channel_state,
            &channel_token,
            cur_state,
            new_state,
            [11u8; 32], // bad pay-token-mask commitment
            rev_lock_com,
            0,
            &mut cust_state,
            ptr::null_mut(),
            None,
            None,
        );
        assert!(res_cust.is_err());

        // wait for mpctest to complete execution
        let ecode = mpc_child.wait().expect("failed to wait on mpctest");
        assert!(ecode.success());

        // load the updated merchant state
        let mut merch_state =
            load_merchant_state_info(&mut db.conn, &db_key, &merch_state_key).unwrap();
        let mask = mpc::pay_confirm_mpc_result(
            &mut db as &mut dyn StateDatabase,
            session_id.clone(),
            res_cust.is_ok(),
            &mut merch_state,
        );
        assert!(mask.is_err());

        let session_id_hex = hex::encode(session_id);
        let session_state = db.load_session_state(&session_id_hex).unwrap();
        print!("Session State: {:?}\n", session_state);
        assert!(session_state.status == PaymentStatus::Error);
    }
}
