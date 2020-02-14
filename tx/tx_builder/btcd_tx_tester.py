
import base58
import bech32
import binascii
import ecdsa
import hashlib
import json
import os
import subprocess
import time


def dSHA256(data):
    hash_1 = hashlib.sha256(data).digest()
    hash_2 = hashlib.sha256(hash_1).digest()
    return hash_2

def hash160(s):
    '''sha256 followed by ripemd160'''
    return hashlib.new('ripemd160', hashlib.sha256(s).digest()).digest()

def privkey_to_pubkey(privkey):
    signing_key = ecdsa.SigningKey.from_string(privkey, curve=ecdsa.SECP256k1) # Don't forget to specify the curve
    verifying_key = signing_key.get_verifying_key()

    # Use this code block if the address you gave corresponds to the compressed public key
    x_cor = bytes.fromhex(verifying_key.to_string().hex())[:32] # The first 32 bytes are the x coordinate
    y_cor = bytes.fromhex(verifying_key.to_string().hex())[32:] # The last 32 bytes are the y coordinate
    if int.from_bytes(y_cor, byteorder="big", signed=True) % 2 == 0: # We need to turn the y_cor into a number.
        public_key = bytes.fromhex("02" + x_cor.hex())
    else:
        public_key = bytes.fromhex("03" + x_cor.hex())
    return public_key


# Functions related to generating bitcoin addresses

def encode_base58(s):
    BASE58_ALPHABET = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
    count = 0
    for c in s:
        if c == 0:
            count += 1
        else:
            break
    num = int.from_bytes(s, 'big')
    prefix = '1' * count
    result = ''
    while num > 0:
        num, mod = divmod(num, 58)
        result = BASE58_ALPHABET[mod] + result
    return prefix + result

def encode_base58_checksum(b):
    return encode_base58(b + dSHA256(b)[:4])

# https://en.bitcoin.it/wiki/Wallet_import_format
def privkey_to_wif(privkey, compressed_pubkey, testnet):
    if testnet:
        prefix = b"\xEF"
    else:
        prefix = b"\x80"

    # if the privkey will correspond to a compressed public key
    if compressed_pubkey:
        extended = prefix + privkey + b"\x01"
    else:
        extended = prefix + privkey

    extendedchecksum = extended + dSHA256(extended)[:4]
    wif = encode_base58(extendedchecksum)

    return wif

# https://learnmeabitcoin.com/guide/wif
def wif_to_privkey(private_key_WIF):
    private_key_full = base58.b58decode(private_key_WIF)

    # If the WIF encoding includes the optional "01" byte for compressed privKey,
    # do not include it in the final output.
    if len(private_key_full) == 38:
        private_key = private_key_full[1:-5]
        print("compressed pubkey")
    else:
        private_key = private_key_full[1:-4]
        print("not compressed pubkey")
    return private_key


def pk_to_p2pkh(compressed, network):
    '''Returns the address string'''
    pk_hash = hash160(compressed)
    if network == "testnet":
        prefix = b'\x6f'
    elif network == "simnet":
        prefix = b'\x3f'
    elif network == "mainnet":
        prefix = b'\x00'
    else:
        return "Enter the network: tesnet/simnet/mainnet"
    return encode_base58_checksum(prefix + pk_hash)

def pk_to_p2sh_p2wpkh(compressed, network):
    pk_hash = hash160(compressed)
    redeemScript = bytes.fromhex(f"0014{pk_hash.hex()}")
    rs_hash = hash160(redeemScript)
    if network == "testnet":
        prefix = b"\xc4"
    elif network == "simnet":
        prefix = b'\x7b'
    elif network == "mainnet":
        prefix = b"\x05"
    else:
        return "Enter the network: tesnet/simnet/mainnet"
    return encode_base58_checksum(prefix + rs_hash)


def pk_to_p2wpkh(compressed, network):
    pk_hash = hash160(compressed)
    redeemScript = bytes.fromhex(f"0014{pk_hash.hex()}")
    spk = binascii.unhexlify(redeemScript.hex())
    version = spk[0] - 0x50 if spk[0] else 0
    program = spk[2:]
    if network == "testnet":
        prefix = 'tb'
    elif network == "simnet":
        prefix = 'sb'
    elif network == "mainnet":
        prefix = 'bc'
    else:
        return "Enter the network: tesnet/simnet/mainnet"
    return bech32.encode(prefix, version, program)


def wif_to_addresses(wif):
#     wif = "cUy9rC6wteKizfu1fgP2abKUWTkJxjqKp2fba91FkU332CFHo6ix"
    privkey = wif_to_privkey(wif)
    public_key = privkey_to_pubkey(privkey)
    p2pkh_address = pk_to_p2pkh(public_key, network = "simnet")
    p2sh_p2wpkh_address = pk_to_p2sh_p2wpkh(public_key, network = "simnet")

    print("WIF Private key: " + wif)
    print("Private key: " + privkey.hex())
    print("Public key: " + public_key.hex())
    print("Public key hash: " + hash160(public_key).hex())
    print("Address: " + p2pkh_address)
    print("Address: " + p2sh_p2wpkh_address)

def privkey_to_addresses(privkey):
    # privkey = bytes.fromhex("AF933A6C602069F1CBC85990DF087714D7E86DF0D0E48398B7D8953E1F03534A")
    public_key = privkey_to_pubkey(privkey)
    p2pkh_address = pk_to_p2pkh(public_key, network = "simnet")
    p2sh_p2wpkh_address = pk_to_p2sh_p2wpkh(public_key, network = "simnet")
    p2wpkh_address = pk_to_p2wpkh(public_key, network = "simnet")

    print("Private key: " + privkey.hex())
    print("Public key: " + public_key.hex())
    print("Public key hash: " + hash160(public_key).hex())
    print("p2pkh_address: " + p2pkh_address)
    print("np2wkh_address: " + p2sh_p2wpkh_address)
    print("p2wpkh_address: " + p2wpkh_address)



# Generate example priv/pubkeys
miner_privkey_hex = "5511111111111111111111111111111100000000000000000000000000000000"
miner_pubkey_bytes = privkey_to_pubkey(bytes.fromhex(miner_privkey_hex))
miner_p2wpkh_address = pk_to_p2wpkh(miner_pubkey_bytes, network = "simnet")
miner_p2sh_p2wpkh_address = pk_to_p2sh_p2wpkh(miner_pubkey_bytes, network = "simnet")

# Make sure btcd is not already running
out = subprocess.getoutput("btcctl --simnet --rpcuser=kek --rpcpass=kek stop")
# if btcd was not running already, it'll return "Post https://localhost:18556: dial tcp [::1]:18556: connect: connection refused"
print(out)

# start up btcd in simnet mode with Alice's address as coinbase tx output
# NOTE: This needs to be run in a separate terminal, otherwise it'll get stuck here
# subprocess.getoutput("btcd --txindex --simnet --rpcuser=kek --rpcpass=kek --miningaddr=" + miner_p2sh_p2wpkh_address)
command = str("btcd --txindex --simnet --rpcuser=kek --rpcpass=kek --minrelaytxfee=0 --miningaddr=" + miner_p2sh_p2wpkh_address)
print("\nExecute this command in a separate terminal, then press Enter to continue\n" + command)
input("Press Enter to continue...")

# generate 1 block to fund Alice
# get block hash to find the coinbase transaction
blockhash = json.loads(subprocess.getoutput("btcctl --simnet --rpcuser=kek --rpcpass=kek generate 1"))
block = json.loads(subprocess.getoutput("btcctl --simnet --rpcuser=kek --rpcpass=kek getblock " + blockhash[0]))
# print("blockhash: " + str(blockhash[0]))
# print("block: " + json.dumps(block, indent=4))

# get the coinbase txid
mined_txid = block["tx"][0]
print("alice's mined_txid: " + mined_txid)

# mine 100 blocks so that the coinbase tx is spendable
subprocess.getoutput("btcctl --simnet --rpcuser=kek --rpcpass=kek generate 100");

escrow_tx = "0200000000010193761230d591a58f2c63367325a1f7210d6bc774e8e240639d71becc7eadc1840000000017160014bb197ac92e740c8a5c06eaf1e197298b0938f59fffffffff028813000000000000220020f59122a8db32dca693570ade36bdaacdf096480311ba14c6a4ea05705606f68778de052a0100000016001461492b43be394b9e6eeb077f17e73665bbfd455b02483045022100c484b066b92b4a317d8276be882fd1949cee6bde7683fadc6515a62769bd710e022039d31170649866cca4a04f23daa3863fd73a687c1581918e653dde54bd288dfe012103343ff7ef1f147c1d9a31fa507f0597c9dc5a4a47760b1aa98445382287b46c0900000000"

escrow_txid = subprocess.getoutput("btcctl --simnet --rpcuser=kek --rpcpass=kek sendrawtransaction " + escrow_tx)
print("escrow_txid: " + escrow_txid)

# mine 3 blocks (waiting for on chain confirmations)
subprocess.getoutput("btcctl --simnet --rpcuser=kek --rpcpass=kek generate 3");

test_close_tx = "merch_close"
if test_close_tx == "merch_close":
    merch_close_tx = "02000000000101c6e46b3dfcfe87c7a9264580d892c02df0fc77dbe8c9f8fba29b310717daeb7a0000000000ffffffff01881300000000000022002044761763544d454b1b4d5431b128d225678ab7cc41577cbff48219c89fc1cff80400473044022079d2a81c1c81d27ec90c2a5c0e0bc1bd58420364eda68eef2fc5b0e912ae368002200cdefef9a9820f573501edaa67f5a4b2d28c8745f0979f9839236d7495f56a5001473044022010dcc06d38009f8a5eb2befdc5274b080e44f844492933d5485f63ec003b6a030220444c1ed9620325530f289a44e22392158d5ccd0a47303a3561a127c8c9284caa0147522103273f139f523a46d50cbdf76eda60818df056e5d14443224891496fdfa5a35ea52103b8fc804c25fc3a4080ce2bcf67d224754e759a8716e79b60be9741d07c2e7de352ae00000000"

    merch_close_txid = subprocess.getoutput("btcctl --simnet --rpcuser=kek --rpcpass=kek sendrawtransaction " + merch_close_tx)
    print("merch_close_txid: " + merch_close_txid)
elif test_close_tx == "cust_close":
    cust_close_tx = "02000000000101bfa0dfbb74083fd5a8d47ca7d61742dbc116d1ce6a1c3bf17b4e3b951b486e320000000000ffffffff0388130000000000002200205a5f438e1e51765254bce06e46c24a5ebb01ae2b503db39e4f6ba4d844403b5a0000000000000000160014f07b9e032209c78b06c133a053e7b322a5683eb10000000000000000436a41ceda909b38c369be8c3e753441e18afc497d62279bbe1adadd228aa5ca3ff3cc034aa1084873b6880ca9f6db899576c40f688edfab1caa747b784f4bd9ffad28560400473045022100d9daf5b7615724ddc43ea46cf34410e3f3d8e6509f54e674c76482453952c479022077669c77fd309e90c8be97841b7b5c7ebcd4f72570928c14c8ca089e5c03cf794830450221008299be569796358c55b36b8123526e44b28a7b0ebb5a1e40e5cf1f0c0e092f0002202a82793d37ff201029ac48762b24990137b7bc8881c5c2060bf834034b7aa6a801475221028d9b9525c490f49c192f24cab2586650072f80e434263c5ceb512bca4c7b0fda2102cdc2963b0a93c310add30f689908b9b3759027a081560a31ef425d6710ae341a52ae00000000"

    cust_close_txid = subprocess.getoutput("btcctl --simnet --rpcuser=kek --rpcpass=kek sendrawtransaction " + cust_close_tx)
    print("cust_close_txid: "+ cust_close_txid)
