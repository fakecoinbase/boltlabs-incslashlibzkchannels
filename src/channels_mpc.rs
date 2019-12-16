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
    pub fn set_customer_pk(&mut self, pk_c: secp256k1::PublicKey) {
        self.pk_c = Some(pk_c);
    }

    pub fn is_init(&self) -> bool {
        return !self.pk_c.is_none();
    }

    pub fn compute_channel_id(&self) -> Result<[u8; 32], String> {
        if self.pk_c.is_none() {
            return Err(String::from("pk_c is not initialized yet"));
        }

        // check txids are set
        let input = serde_json::to_vec(&self).unwrap();

        return Ok(hash_to_slice(&input));
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
    state: Option<State>, // vector of field elements that represent current state
    index: i32,
    close_signatures: HashMap<i32, secp256k1::Signature>,
    pay_tokens: HashMap<i32, secp256k1::Signature>
}

const NONCE_LEN: usize = 12;

#[cfg(feature = "mpc-bitcoin")]
impl CustomerMPCState {
    pub fn new<R: Rng>(csprng: &mut R, cust_bal: i64, merch_bal: i64, name: String) -> Self
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

        // pick random t
        let mut t: [u8; 32] = [0; 32];
        //csprng.fill_bytes(&mut t);

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
            state: None,
            index: 0,
            close_signatures: ct_db,
            pay_tokens: pt_db,
        };
    }

    pub fn generate_init_state<R: Rng>(&mut self, csprng: &mut R, channel_token: &mut ChannelMPCToken) {
        assert!(self.state.is_none());

        let mut nonce: [u8; NONCE_LEN] = [0; NONCE_LEN];
        csprng.fill_bytes(&mut nonce);

        channel_token.set_customer_pk(self.pk_c.clone());

        let state = State { nonce: nonce, rev_lock: self.rev_lock, pk_c: self.pk_c, pk_m: channel_token.pk_m.clone(),
                                bc: self.cust_balance, bm: self.merch_balance, escrow_txid: channel_token.escrow_txid,
                                merch_txid: channel_token.merch_txid };

        // generate initial commitment to state of channel
        // let s_com = state.generate_commitment(&t);

        assert!(channel_token.is_init());
        self.state = Some(state);
    }

    pub fn generate_init_commitment<R: Rng>(&mut self, csprng: &mut R) -> [u8; 32] {
        assert!(!self.state.is_none());
        let mut t: [u8; 32] = [0; 32];
        csprng.fill_bytes(&mut t);
        self.t.copy_from_slice(&t);
        return self.state.unwrap().generate_commitment(&self.t);
    }

    pub fn get_current_state(&self) -> State {
        assert!(self.state.is_some());
        return self.state.unwrap();
    }


    pub fn generate_init_channel_token(&self, pk_m: &secp256k1::PublicKey, escrow_txid: [u8; 32], merch_txid: [u8; 32]) -> ChannelMPCToken {

        return ChannelMPCToken {
            pk_c: Some(self.pk_c.clone()),
            pk_m: pk_m.clone(),
            escrow_txid: escrow_txid,
            merch_txid: merch_txid
        };
    }

    pub fn get_close_signature(&self) -> secp256k1::Signature {
        let index = self.index;
        let close_signature = self.close_signatures.get(&index).unwrap();
        // rerandomize first
        return close_signature.clone();
    }

    // verify the closing
    pub fn verify_close_signature(&mut self, channel: &ChannelMPCState, close_sig: &secp256k1::Signature) -> bool {
        println!("verify_close_signature - State: {}", &self.state.unwrap());
        let is_close_valid = true;
        //println!("Customer - Verification failed for close token!");
        return is_close_valid;
    }

    pub fn verify_pay_signature(&mut self, channel: &ChannelMPCState, pay_sig: &secp256k1::Signature) -> bool {
        println!("verify_pay_signature - State: {}", &self.state.unwrap());
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
    pub activate_map: HashMap<String, State>,
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
            activate_map: HashMap::new(),
            lock_map: HashMap::new(),
            pay_tokens: HashMap::new(),
        }, ch)
    }

    pub fn get_public_key(&mut self) -> secp256k1::PublicKey {
        return self.pk_m.clone();
    }

    pub fn activate_channel(&self, channel_token: &ChannelMPCToken, s_com: [u8; 32]) -> secp256k1::Signature {
        // store the state inside the ActivateBucket
        let channel_id = channel_token.compute_channel_id().unwrap();
        let channel_id_str= hex::encode(channel_id.to_vec());

        // does MPC verify that s_com was generated from s_0 in activate bucket?

        // TODO: figure out how we are generating this (w/ or w/o MPC)?
        let secp = secp256k1::Secp256k1::signing_only();
        let msg = secp256k1::Message::from_slice(&s_com).unwrap();
        let pay_sig = secp.sign(&msg, &self.sk_m);
        return pay_sig;

    }

    pub fn store_initial_state(&mut self, channel_token: &ChannelMPCToken, s0: &State) -> bool {
        let channel_id = channel_token.compute_channel_id().unwrap();
        let channel_id_str= hex::encode(channel_id.to_vec());
        self.activate_map.insert(channel_id_str, s0.clone());

        return true;
    }

    pub fn unlink_channel(&self, ) -> bool {
        return true;
    }
}

#[cfg(feature = "mpc-bitcoin")]
#[cfg(test)]
mod tests {
    use super::*;
    use channels_mpc::{ChannelMPCState, MerchantMPCState, CustomerMPCState};

    fn generate_test_txs<R: Rng>(csprng: &mut R) -> ([u8; 32], [u8; 32]) {
        let mut txid1 = [0u8; 32];
        let mut txid2 = [0u8; 32];

        csprng.fill_bytes(&mut txid1);
        csprng.fill_bytes(&mut txid2);

        println!("Escrow txid: {:?}", txid1);
        println!("Merch txid: {:?}", txid2);

        return (txid1, txid2)
    }

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

        // initialize on the customer side with balance: b0_cust
        let mut cust_state = CustomerMPCState::new(rng, b0_cust, b0_merch, String::from("Alice"));

        // at this point, cust/merch have both exchanged initial sigs (escrow-tx + merch-close-tx)
        let (escrow_txid, merch_txid) = generate_test_txs(rng);

        // initialize the channel token on with pks
        let mut channel_token = cust_state.generate_init_channel_token(&merch_state.pk_m, escrow_txid, merch_txid);

        // generate and send initial state to the merchant
        cust_state.generate_init_state(rng, &mut channel_token);
        let s_0 = cust_state.get_current_state();

        println!("Begin activate phase for channel");

        let s_com = cust_state.generate_init_commitment(rng);

        // send the initial state s_0 to merchant
        merch_state.store_initial_state(&channel_token, &s_0);

        // activate channel
        let pay_sig = merch_state.activate_channel(&channel_token, s_com);

        println!("init commitment => {:?}", s_com);

        println!("Signature on s_com => {:?}", pay_sig);

        // now customer can unlink by making a first payment
        // let sig = merch_state.unlink_channel(&channel_token, s_0);

        // assert!(cust_state.verify_pay_signature(&channel, &pay_sig));

    }
}
