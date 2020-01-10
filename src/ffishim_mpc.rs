#[cfg(feature = "mpc-bitcoin")]
#[no_mangle]
pub mod ffishim_mpc {
    extern crate libc;

    use mpc;

    use serde::Deserialize;

    use libc::c_char;
    use std::ffi::{CStr, CString};
    use std::str;
    use channels_mpc::{CustomerMPCState, MerchantMPCState, ChannelMPCToken, ChannelMPCState, MaskedTxMPCInputs};
    use wallet::State;
    use hex::FromHexError;
    use mpc::RevokedState;

    fn error_message(s: String) -> *mut c_char {
        let ser = ["{\'error\':\'", &s, "\'}"].concat();
        let cser = CString::new(ser).unwrap();
        cser.into_raw()
    }

    macro_rules! bolt_try {
        ($e:expr) => (match $e {
            Ok(val) => val.unwrap(),
            Err(err) => return error_message(err),
        });
    }

    macro_rules! handle_errors {
        ($e:expr) => (match $e {
            Ok(val) => val,
            Err(err) => return error_message(err.to_string()),
        });
    }

    pub type ResultSerdeType<T> = Result<T, serde_json::error::Error>;

    fn deserialize_result_object<'a, T>(serialized: *mut c_char) -> ResultSerdeType<T>
        where
            T: Deserialize<'a>,
    {
        let bytes = unsafe { CStr::from_ptr(serialized).to_bytes() };
        let string: &str = str::from_utf8(bytes).unwrap(); // make sure the bytes are UTF-8
        serde_json::from_str(&string)
    }

    fn deserialize_hex_string(serialized: *mut c_char) -> Result<Vec<u8>, FromHexError>
    {
        let bytes = unsafe { CStr::from_ptr(serialized).to_bytes() };
        let string: &str = str::from_utf8(bytes).unwrap(); // make sure the bytes are UTF-8
        hex::decode(&string)
    }

    #[no_mangle]
    pub extern fn mpc_free_string(pointer: *mut c_char) {
        unsafe {
            if pointer.is_null() { return; }
            CString::from_raw(pointer)
        };
    }

    #[no_mangle]
    pub extern fn mpc_channel_setup(channel_name: *const c_char, third_party_support: u32) -> *mut c_char {
        let bytes = unsafe { CStr::from_ptr(channel_name).to_bytes() };
        let name: &str = str::from_utf8(bytes).unwrap(); // make sure the bytes are UTF-8

        let mut tps = false;
        if third_party_support >= 1 {
            tps = true;
        }
        let channel_state = mpc::ChannelMPCState::new(name.to_string(), tps);

        let ser = ["{\'channel_state\':\'", serde_json::to_string(&channel_state).unwrap().as_str(), "\'}"].concat();
        let cser = CString::new(ser).unwrap();
        cser.into_raw()
    }

    // INIT

    #[no_mangle]
    pub extern fn mpc_init_merchant(ser_channel_state: *mut c_char, name_ptr: *const c_char) -> *mut c_char {
        let rng = &mut rand::thread_rng();
        let channel_state_result: ResultSerdeType<mpc::ChannelMPCState> = deserialize_result_object(ser_channel_state);
        let mut channel_state = handle_errors!(channel_state_result);

        let bytes = unsafe { CStr::from_ptr(name_ptr).to_bytes() };
        let name: &str = str::from_utf8(bytes).unwrap(); // make sure the bytes are UTF-8

        let merch_state = mpc::init_merchant(rng, &mut channel_state, name);

        let ser = ["{\'merch_state\':\'", serde_json::to_string(&merch_state).unwrap().as_str(), "\', \'channel_state\':\'", serde_json::to_string(&channel_state).unwrap().as_str(), "\'}"].concat();

        let cser = CString::new(ser).unwrap();
        cser.into_raw()
    }

    #[no_mangle]
    pub extern fn mpc_init_customer(ser_pk_m: *mut c_char, ser_tx: *mut c_char, balance_customer: i64, balance_merchant: i64, name_ptr: *const c_char) -> *mut c_char {
        let rng = &mut rand::thread_rng();

        // Deserialize the pk_m
        let pk_m_result: ResultSerdeType<secp256k1::PublicKey> = deserialize_result_object(ser_pk_m);
        let pk_m = handle_errors!(pk_m_result);

        // Deserialize the tx
        let tx_result: ResultSerdeType<mpc::FundingTxInfo> = deserialize_result_object(ser_tx);
        let tx = handle_errors!(tx_result);

        // Deserialize the name
        let bytes = unsafe { CStr::from_ptr(name_ptr).to_bytes() };
        let name: &str = str::from_utf8(bytes).unwrap(); // make sure the bytes are UTF-8

        // We change the channel state
        let (channel_token, cust_state) = mpc::init_customer(rng, &pk_m, tx, balance_customer, balance_merchant, name);
        let ser = ["{\'cust_state\':\'", serde_json::to_string(&cust_state).unwrap().as_str(), "\', \'channel_token\':\'", serde_json::to_string(&channel_token).unwrap().as_str(), "\'}"].concat();
        let cser = CString::new(ser).unwrap();
        cser.into_raw()
    }

    // ACTIVATE

    #[no_mangle]
    pub extern fn mpc_activate_customer(ser_cust_state: *mut c_char) -> *mut c_char {
        let rng = &mut rand::thread_rng();

        // Deserialize the cust_state
        let cust_state_result: ResultSerdeType<CustomerMPCState> = deserialize_result_object(ser_cust_state);
        let mut cust_state = handle_errors!(cust_state_result);

        // We change the channel state
        let state = mpc::activate_customer(rng, &mut cust_state);
        let ser = ["{\'state\':\'", serde_json::to_string(&state).unwrap().as_str(), "\', \'cust_state\':\'", serde_json::to_string(&cust_state).unwrap().as_str(), "\'}"].concat();
        let cser = CString::new(ser).unwrap();
        cser.into_raw()
    }

    #[no_mangle]
    pub extern fn mpc_activate_merchant(ser_channel_token: *mut c_char, ser_state: *mut c_char, ser_merch_state: *mut c_char) -> *mut c_char {
        // Deserialize the ChannelToken
        let channel_token_result: ResultSerdeType<ChannelMPCToken> = deserialize_result_object(ser_channel_token);
        let mut channel_token = handle_errors!(channel_token_result);

        // Deserialize the state
        let state_result: ResultSerdeType<State> = deserialize_result_object(ser_state);
        let mut state = handle_errors!(state_result);

        // Deserialize the merch_state
        let merch_state_result: ResultSerdeType<MerchantMPCState> = deserialize_result_object(ser_merch_state);
        let mut merch_state = handle_errors!(merch_state_result);

        // We change the channel state
        let pay_token = mpc::activate_merchant(channel_token, &state, &mut merch_state);
        let ser = ["{\'pay_token\':\'", &hex::encode(pay_token), "\', \'merch_state\':\'", serde_json::to_string(&merch_state).unwrap().as_str(), "\'}"].concat();
        let cser = CString::new(ser).unwrap();
        cser.into_raw()
    }

    #[no_mangle]
    pub extern fn mpc_activate_customer_finalize(ser_pay_token: *mut c_char, ser_cust_state: *mut c_char) -> *mut c_char {
        // Deserialize the cust_state
        let cust_state_result: ResultSerdeType<CustomerMPCState> = deserialize_result_object(ser_cust_state);
        let mut cust_state = handle_errors!(cust_state_result);

        // Deserialize pay token
        let pay_token_result = deserialize_hex_string(ser_pay_token);
        let pay_token = handle_errors!(pay_token_result);
        let mut pay_token_0 = [0u8; 32];
        pay_token_0.copy_from_slice(pay_token.as_slice());

        // We change the channel state
        mpc::activate_customer_finalize(pay_token_0, &mut cust_state);
        let ser = ["{\'cust_state\':\'", serde_json::to_string(&cust_state).unwrap().as_str(), "\'}"].concat();
        let cser = CString::new(ser).unwrap();
        cser.into_raw()
    }

    // PAYMENT

    #[no_mangle]
    pub extern fn mpc_prepare_payment_customer(ser_channel_state: *mut c_char, amount: i64, ser_cust_state: *mut c_char) -> *mut c_char {
        let rng = &mut rand::thread_rng();

        // Deserialize the channel_state
        let channel_state_result: ResultSerdeType<ChannelMPCState> = deserialize_result_object(ser_channel_state);
        let mut channel_state = handle_errors!(channel_state_result);

        // Deserialize the cust_state
        let cust_state_result: ResultSerdeType<CustomerMPCState> = deserialize_result_object(ser_cust_state);
        let mut cust_state = handle_errors!(cust_state_result);

        // We change the channel state
        let (state, rev_lock_com, rev_lock, rev_secret) = mpc::pay_prepare_customer(rng, &mut channel_state, amount, &mut cust_state);
        let ser = ["{\'rev_lock_com\':\'", &hex::encode(rev_lock_com), "\', \'rev_lock\':\'", &hex::encode(rev_lock), "\', \'rev_secret\':\'", &hex::encode(rev_secret), "\', \'state\':\'", serde_json::to_string(&state).unwrap().as_str(), "\', \'channel_state\':\'", serde_json::to_string(&channel_state).unwrap().as_str(), "\', \'cust_state\':\'", serde_json::to_string(&cust_state).unwrap().as_str(), "\'}"].concat();
        let cser = CString::new(ser).unwrap();
        cser.into_raw()
    }

    #[no_mangle]
    pub extern fn mpc_prepare_payment_merchant(ser_nonce: *mut c_char, ser_merch_state: *mut c_char) -> *mut c_char {
        let rng = &mut rand::thread_rng();

        // Deserialize nonce
        let nonce_result: ResultSerdeType<Vec<u8>> = deserialize_result_object(ser_nonce);
        let nonce = handle_errors!(nonce_result);
        let mut nonce_ar = [0u8; 16];
        nonce_ar.copy_from_slice(nonce.as_slice());

        // Deserialize the merch_state
        let merch_state_result: ResultSerdeType<MerchantMPCState> = deserialize_result_object(ser_merch_state);
        let mut merch_state = handle_errors!(merch_state_result);

        // We change the channel state
        let pay_token_mask_com = mpc::pay_prepare_merchant(rng, nonce_ar, &mut merch_state);
        let ser = ["{\'pay_token_mask_com\':\'", &hex::encode(pay_token_mask_com), "\', \'merch_state\':\'", serde_json::to_string(&merch_state).unwrap().as_str(), "\'}"].concat();
        let cser = CString::new(ser).unwrap();
        cser.into_raw()
    }

    #[no_mangle]
    pub extern fn mpc_pay_customer(ser_channel_state: *mut c_char, ser_channel_token: *mut c_char, ser_start_state: *mut c_char, ser_end_state: *mut c_char, ser_pay_token_mask_com: *mut c_char, ser_rev_lock_com: *mut c_char, amount: i64, ser_cust_state: *mut c_char) -> *mut c_char {
        // Deserialize the channel_state
        let channel_state_result: ResultSerdeType<ChannelMPCState> = deserialize_result_object(ser_channel_state);
        let mut channel_state = handle_errors!(channel_state_result);

        // Deserialize the ChannelToken
        let channel_token_result: ResultSerdeType<ChannelMPCToken> = deserialize_result_object(ser_channel_token);
        let mut channel_token = handle_errors!(channel_token_result);

        // Deserialize the start_state
        let start_state_result: ResultSerdeType<State> = deserialize_result_object(ser_start_state);
        let mut start_state = handle_errors!(start_state_result);

        // Deserialize the end_state
        let end_state_result: ResultSerdeType<State> = deserialize_result_object(ser_end_state);
        let mut end_state = handle_errors!(end_state_result);

        // Deserialize pay_token_mask_com
        let pay_token_mask_com_result = deserialize_hex_string(ser_pay_token_mask_com);
        let pay_token_mask_com = handle_errors!(pay_token_mask_com_result);
        let mut pay_token_mask_com_ar = [0u8; 32];
        pay_token_mask_com_ar.copy_from_slice(pay_token_mask_com.as_slice());

        // Deserialize rev_lock_com
        let rev_lock_com_result = deserialize_hex_string(ser_rev_lock_com);
        let rev_lock_com = handle_errors!(rev_lock_com_result);
        let mut rev_lock_com_ar = [0u8; 32];
        rev_lock_com_ar.copy_from_slice(rev_lock_com.as_slice());

        // Deserialize the cust_state
        let cust_state_result: ResultSerdeType<CustomerMPCState> = deserialize_result_object(ser_cust_state);
        let mut cust_state = handle_errors!(cust_state_result);

        // We change the channel state
        let result = mpc::pay_customer(&mut channel_state, channel_token, start_state, end_state, pay_token_mask_com_ar, rev_lock_com_ar, amount, &mut cust_state);
        let is_ok: bool = handle_errors!(result);
        let ser = ["{\'is_ok\':", &is_ok.to_string(), ", \'cust_state\':\'", serde_json::to_string(&cust_state).unwrap().as_str(), "\'}"].concat();
        let cser = CString::new(ser).unwrap();
        cser.into_raw()
    }

    #[no_mangle]
    pub extern fn mpc_pay_merchant(ser_channel_state: *mut c_char, ser_nonce: *mut c_char, ser_pay_token_mask_com: *mut c_char, ser_rev_lock_com: *mut c_char, amount: i64, ser_merch_state: *mut c_char) -> *mut c_char {
        let rng = &mut rand::thread_rng();

        // Deserialize the channel_state
        let channel_state_result: ResultSerdeType<ChannelMPCState> = deserialize_result_object(ser_channel_state);
        let mut channel_state = handle_errors!(channel_state_result);

        // Deserialize nonce
        let nonce_result: ResultSerdeType<Vec<u8>> = deserialize_result_object(ser_nonce);
        let nonce = handle_errors!(nonce_result);
        let mut nonce_ar = [0u8; 16];
        nonce_ar.copy_from_slice(nonce.as_slice());

        // Deserialize pay_token_mask_com
        let pay_token_mask_com_result = deserialize_hex_string(ser_pay_token_mask_com);
        let pay_token_mask_com = handle_errors!(pay_token_mask_com_result);
        let mut pay_token_mask_com_ar = [0u8; 32];
        pay_token_mask_com_ar.copy_from_slice(pay_token_mask_com.as_slice());

        // Deserialize rev_lock_com
        let rev_lock_com_result = deserialize_hex_string(ser_rev_lock_com);
        let rev_lock_com = handle_errors!(rev_lock_com_result);
        let mut rev_lock_com_ar = [0u8; 32];
        rev_lock_com_ar.copy_from_slice(rev_lock_com.as_slice());

        // Deserialize the merch_state
        let merch_state_result: ResultSerdeType<MerchantMPCState> = deserialize_result_object(ser_merch_state);
        let mut merch_state = handle_errors!(merch_state_result);

        // We change the channel state
        let result = mpc::pay_merchant(rng, &mut channel_state, nonce_ar, pay_token_mask_com_ar, rev_lock_com_ar, amount, &mut merch_state);
        let masked_tx_inputs = handle_errors!(result);
        let ser = ["{\'masked_tx_inputs\':\'", serde_json::to_string(&masked_tx_inputs).unwrap().as_str(), "\', \'merch_state\':\'", serde_json::to_string(&merch_state).unwrap().as_str(), "\'}"].concat();
        let cser = CString::new(ser).unwrap();
        cser.into_raw()
    }

    #[no_mangle]
    pub extern fn mpc_pay_unmask_tx_customer(ser_masked_tx_inputs: *mut c_char, ser_cust_state: *mut c_char) -> *mut c_char {
        // Deserialize masked_tx_inputs
        let masked_tx_inputs_result: ResultSerdeType<MaskedTxMPCInputs> = deserialize_result_object(ser_masked_tx_inputs);
        let mut masked_tx_inputs = handle_errors!(masked_tx_inputs_result);

        // Deserialize the cust_state
        let cust_state_result: ResultSerdeType<CustomerMPCState> = deserialize_result_object(ser_cust_state);
        let mut cust_state = handle_errors!(cust_state_result);

        // We change the channel state
        let is_ok = mpc::pay_unmask_tx_customer(masked_tx_inputs, &mut cust_state);
        let ser = ["{\'is_ok\':", &is_ok.to_string(), ", \'cust_state\':\'", serde_json::to_string(&cust_state).unwrap().as_str(), "\'}"].concat();
        let cser = CString::new(ser).unwrap();
        cser.into_raw()
    }

    #[no_mangle]
    pub extern fn mpc_pay_validate_rev_lock_merchant(ser_revoked_state: *mut c_char, ser_merch_state: *mut c_char) -> *mut c_char {
        // Deserialize masked_tx_inputs
        let revoked_state_result: ResultSerdeType<RevokedState> = deserialize_result_object(ser_revoked_state);
        let mut revoked_state = handle_errors!(revoked_state_result);

        // Deserialize the merch_state
        let merch_state_result: ResultSerdeType<MerchantMPCState> = deserialize_result_object(ser_merch_state);
        let mut merch_state = handle_errors!(merch_state_result);

        // We change the channel state
        let pay_token_mask_result = mpc::pay_validate_rev_lock_merchant(revoked_state, &mut merch_state);
        let mut pay_token_mask = handle_errors!(pay_token_mask_result);
        let ser = ["{\'pay_token_mask\':\'", &hex::encode(pay_token_mask), "\', \'merch_state\':\'", serde_json::to_string(&merch_state).unwrap().as_str(), "\'}"].concat();
        let cser = CString::new(ser).unwrap();
        cser.into_raw()
    }

    #[no_mangle]
    pub extern fn mpc_pay_unmask_pay_token_customer(ser_pt_mask_bytes: *mut c_char, ser_cust_state: *mut c_char) -> *mut c_char {
        // Deserialize pt_mask_bytes
        let pt_mask_bytes_result = deserialize_hex_string(ser_pt_mask_bytes);
        let pt_mask_bytes = handle_errors!(pt_mask_bytes_result);
        let mut pt_mask_bytes_ar = [0u8; 32];
        pt_mask_bytes_ar.copy_from_slice(pt_mask_bytes.as_slice());

        // Deserialize the cust_state
        let cust_state_result: ResultSerdeType<CustomerMPCState> = deserialize_result_object(ser_cust_state);
        let mut cust_state = handle_errors!(cust_state_result);

        // We change the channel state
        let is_ok = mpc::pay_unmask_pay_token_customer(pt_mask_bytes_ar, &mut cust_state);
        let ser = ["{\'is_ok\':", &is_ok.to_string(), ", \'cust_state\':\'", serde_json::to_string(&cust_state).unwrap().as_str(), "\'}"].concat();
        let cser = CString::new(ser).unwrap();
        cser.into_raw()
    }
}