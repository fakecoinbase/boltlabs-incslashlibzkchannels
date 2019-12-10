use super::*;
use rand::Rng;
use wallet::State;
use util::{hash_to_slice};

// PROTOTYPE
#[cfg(feature = "mpc-bitcoin")]
#[derive(Clone, Serialize, Deserialize)]
pub struct ChannelMPCToken {
    pub pk_c: Option<secp256k1::PublicKey>, // pk_c
    pub pk_m: secp256k1::PublicKey, // pk_m
    pub escrow_txid: [u8; 32],
    pub merch_txid: [u8; 32]
}

#[cfg(feature = "mpc-bitcoin")]
impl ChannelMPCToken {
    pub fn set_customer_pk(&mut self, pk_c: &secp256k1::PublicKey) {
        self.pk_c = Some(pk_c.clone());
    }

    pub fn is_init(&self) -> bool {
        return !self.pk_c.is_none();
    }

    pub fn compute_channel_id(&self) -> [u8; 32] {
        if self.pk_c.is_none() {
            panic!("pk_c is not initialized yet");
        }

        // check txids are set
        let input = serde_json::to_vec(&self).unwrap();

        return hash_to_slice(&input);
    }

    // add a method to compute hash on chain: SHA256 + RIPEMD160?
}


#[cfg(feature = "mpc-bitcoin")]
#[derive(Clone, Serialize, Deserialize)]
pub struct ChannelMPCState {
    R: i32,
    tx_fee: i64,
    pub name: String,
    pub pay_init: bool,
    pub channel_established: bool,
    pub third_party: bool,
}

#[cfg(feature = "mpc-bitcoin")]
impl ChannelMPCState {
    pub fn new(name: String, third_party_support: bool) -> ChannelMPCState {
        ChannelMPCState {
            R: 0,
            tx_fee: 0,
            name: name.to_string(),
            pay_init: false,
            channel_established: false,
            third_party: third_party_support,
        }
    }

    ///
    /// keygen - takes as input public parameters and generates a digital signature keypair
    ///
//    pub fn keygen<R: Rng>(&mut self, csprng: &mut R, _id: String) -> (secp256k1::SecretKey, secp256k1::PublicKey) {
//        let mut kp = secp256k1::Secp256k1::new();
//        kp.randomize(csprng);
//        let (sk, pk) = kp.generate_keypair(csprng);
//        return (sk, pk);
//    }

    pub fn set_channel_fee(&mut self, fee: i64) {
        self.tx_fee = fee;
    }

    pub fn get_channel_fee(&self) -> i64 {
        return self.tx_fee as i64;
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct LockPreimagePair {
    old_rev_secret: [u8; 32],
    old_rev_lock: [u8; 32]
}

#[cfg(feature = "mpc-bitcoin")]
#[derive(Clone, Serialize, Deserialize)]
pub struct CustomerMPCState {
    pub name: String,
    pub pk_c: secp256k1::PublicKey,
    sk_c: secp256k1::SecretKey,
    pub cust_balance: i64,
    pub merch_balance: i64,
    pub rev_lock: [u8; 32],
    rev_secret: [u8; 32],
    old_kp: Option<LockPreimagePair>, // old lock and preimage pair
    t: [u8; 32], // randomness used to form the commitment
    state: State, // vector of field elements that represent current state
    pub s_com: [u8; 32], // commitment to the current state
    index: i32,
    close_signatures: HashMap<i32, secp256k1::Signature>,
    pay_tokens: HashMap<i32, secp256k1::Signature>
}

const NONCE_LEN: usize = 12;

#[cfg(feature = "mpc-bitcoin")]
impl CustomerMPCState {
    pub fn new<R: Rng>(csprng: &mut R, channel_token: &mut ChannelMPCToken, cust_bal: i64, merch_bal: i64, name: String) -> Self
    {
        let secp = secp256k1::Secp256k1::new();

        let mut seckey = [0u8; 32];
        csprng.fill_bytes(&mut seckey);

        // generate the signing keypair for the channel
        let sk_c = secp256k1::SecretKey::from_slice(&seckey).unwrap();
        let pk_c = secp256k1::PublicKey::from_secret_key(&secp, &sk_c);

        // generate the keypair for the initial state of channel
        let mut rev_secret = [0u8; 32];
        csprng.fill_bytes(&mut rev_secret);

        // compute hash of the revocation secret
        let rev_lock = hash_to_slice(&rev_secret.to_vec());

        channel_token.set_customer_pk(&pk_c);

        // pick random t
        let mut t: [u8; 32] = [0; 32];
        let mut nonce: [u8; NONCE_LEN] = [0; NONCE_LEN];
        csprng.fill_bytes(&mut t);
        csprng.fill_bytes(&mut nonce);

        let state = State { nonce: nonce, rev_lock: rev_lock, pk_c: pk_c, pk_m: channel_token.pk_m.clone(),
                                bc: cust_bal, bm: merch_bal, escrow_txid: channel_token.escrow_txid.clone(),
                                merch_txid: channel_token.merch_txid.clone() };

        // generate initial commitment to state of channel
        let s_com = state.generate_commitment(&t);
        assert!(channel_token.is_init());

        let ct_db = HashMap::new();
        let pt_db = HashMap::new();

        return CustomerMPCState {
            name: name,
            pk_c: pk_c,
            sk_c: sk_c,
            cust_balance: cust_bal,
            merch_balance: merch_bal,
            rev_lock: rev_lock,
            rev_secret: rev_secret,
            old_kp: None,
            t: t,
            state: state,
            s_com: s_com,
            index: 0,
            close_signatures: ct_db,
            pay_tokens: pt_db,
        };
    }

    pub fn get_current_state(&self) -> State {
        return self.state.clone();
    }

    pub fn get_close_signature(&self) -> secp256k1::Signature {
        let index = self.index;
        let close_signature = self.close_signatures.get(&index).unwrap();
        // rerandomize first
        return close_signature.clone();
    }

    // verify the closing
    pub fn verify_close_signature(&mut self, channel: &ChannelMPCState, close_sig: &secp256k1::Signature) -> bool {
        println!("verify_close_signature - State: {}", &self.state);
        let is_close_valid = true;
        //println!("Customer - Verification failed for close token!");
        return is_close_valid;
    }

    pub fn verify_pay_signature(&mut self, channel: &ChannelMPCState, pay_sig: &secp256k1::Signature) -> bool {
        println!("verify_pay_signature - State: {}", &self.state);
        let is_pay_valid = true;
        //println!("Customer - Verification failed for pay token!");
        return is_pay_valid;
    }

    pub fn has_tokens(&self) -> bool {
        let index = self.index;
        let is_ct = self.close_signatures.get(&index).is_some();
        let is_pt = self.pay_tokens.get(&index).is_some();
        return is_ct && is_pt;
    }

    // update the internal state of the customer wallet
    pub fn update(&mut self, new_wallet: CustomerMPCState) -> bool {
        // update everything except for the wpk/wsk pair
        assert!(self.name == new_wallet.name);
        self.cust_balance = new_wallet.cust_balance;
        self.merch_balance = new_wallet.merch_balance;
        self.old_kp = new_wallet.old_kp;
        self.index = new_wallet.index;
        self.close_signatures = new_wallet.close_signatures;
        self.pay_tokens = new_wallet.pay_tokens;

        return true;
    }

//    pub fn generate_revoke_token(&mut self, channel: &ChannelMPCState, close_sig: &secp256k1::Signature) -> ResultBoltType<(RevokedMessage, secp256k1::Signature)> {
//        if self.verify_close_signature(channel, close_sig) {
//            let old_state = self.old_kp.unwrap();
//            // proceed with generating the close token
//            let secp = secp256k1::Secp256k1::new();
//            let rm = RevokedMessage::new(String::from("revoked"), old_state.wpk);
//            let revoke_msg = secp256k1::Message::from_slice(&rm.hash_to_slice()).unwrap();
//            // msg = "revoked"|| old wsk (for old wallet)
//            let revoke_sig = secp.sign(&revoke_msg, &old_state.wsk);
//
//            return Ok((rm, revoke_token));
//        }
//
//        Err(BoltError::new("generate_revoke_token - could not verify the close token."))
//    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LockMap {
    pub lock: [u8; 32],
    pub secret: [u8; 32],
}

#[cfg(feature = "mpc-bitcoin")]
#[derive(Clone, Serialize, Deserialize)]
pub struct MerchantMPCState {
    id: String,
    pk_m: secp256k1::PublicKey, // pk_m
    sk_m: secp256k1::SecretKey, // sk_m
    pub lock_map: HashMap<String, LockMap>,
    pub pay_tokens: HashMap<String, secp256k1::Signature>,
}

#[cfg(feature = "mpc-bitcoin")]
impl MerchantMPCState {
    pub fn new<R: Rng>(csprng: &mut R, channel: &mut ChannelMPCState, id: String) -> (Self, ChannelMPCState) {
        let secp = secp256k1::Secp256k1::new();
        let mut seckey = [0u8; 32];
        csprng.fill_bytes(&mut seckey);

        // generate the signing keypair for the channel
        let sk_m = secp256k1::SecretKey::from_slice(&seckey).unwrap();
        let pk_m = secp256k1::PublicKey::from_secret_key(&secp, &sk_m);

        let mut ch = channel.clone();

        (MerchantMPCState {
            id: id.clone(),
            pk_m: pk_m,
            sk_m: sk_m,
            lock_map: HashMap::new(),
            pay_tokens: HashMap::new(),
        }, ch)
    }

    pub fn init(&mut self, channel: &mut ChannelMPCState) -> ChannelMPCToken {

        return ChannelMPCToken {
            pk_c: None,
            pk_m: self.pk_m.clone(),
            escrow_txid: [0u8; 32],
            merch_txid: [0u8; 32]
        };
    }

    pub fn establish_pay_signature<R: Rng>(&mut self, csprng: &mut R, channel_token: &ChannelMPCToken, s_com: &[u8; 32]) -> secp256k1::Signature {
        // TODO: figure out how we are generating this (w/ or w/o MPC)?
        let secp = secp256k1::Secp256k1::signing_only();
        let msg = secp256k1::Message::from_slice(s_com).unwrap();
        let pay_sig = secp.sign(&msg, &self.sk_m);

        // store the state inside the ActivateBucket

        return pay_sig;
    }
}

#[cfg(feature = "mpc-bitcoin")]
#[cfg(test)]
mod tests {
    use super::*;
    use channels_mpc::{ChannelMPCState, MerchantMPCState, CustomerMPCState};

    #[test]
    fn mpc_channel_util_works() {
        let mut channel = ChannelMPCState::new(String::from("Channel A <-> B"), false);
        let rng = &mut rand::thread_rng();

        let b0_cust = 100;
        let b0_merch = 20;
        // each party executes the init algorithm on the agreed initial challenge balance
        // in order to derive the channel tokens
        // initialize on the merchant side with balance: b0_merch
        let (mut merch_state, mut channel) = MerchantMPCState::new(rng, &mut channel, String::from("Merchant B"));

        // initialize the channel token on merchant side with pks
        let mut channel_token = merch_state.init(&mut channel);

        // at this point, cust/merch have both exchanged initial sigs (escrow-tx + merch-close-tx)

        // initialize on the customer side with balance: b0_cust
        let mut cust_state = CustomerMPCState::new(rng, &mut channel_token, b0_cust, b0_merch, String::from("Alice"));

        let s_0 = cust_state.get_current_state();

        println!("Begin activate phase for channel");

        let pay_sig = merch_state.establish_pay_signature(rng, &mut channel_token, &cust_state.s_com);

        // now customer can unlink by making a first payment

        assert!(cust_state.verify_pay_signature(&channel, &pay_sig));

    }
}
