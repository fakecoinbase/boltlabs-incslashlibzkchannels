# p2wsh input (2-of-2 multisig)
# p2wsh output

import argparse
import hashlib
import ecdsa

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

################################

parser = argparse.ArgumentParser()
parser.add_argument("--cust_privkey", "-csk", help="private key of customer for escrow", required=True)
parser.add_argument("--merch_privkey", "-msk", help="private key of merchant for escrow", required=True)
parser.add_argument("--merch_close_pubkey", "-mcpk", help="public key of merchant close to-self output", required=True)
parser.add_argument("--to_self_delay", "-tsd", help="to_self_delay (in unit of blocks) for the merchant's to-self output", required=True)
parser.add_argument("--txid", "-tx", help="txid of outpoint as hex string", required=True)
parser.add_argument("--index", "-ind", help="index of outpoint (default=0)", default=0, required=False)
parser.add_argument("--amount_btc", "-a", help="amount of btc in", required=True)
parser.add_argument("--output_btc", "-o", help="btc to output", required=True)
parser.add_argument("--verbose", "-v", help="increase output verbosity", action="store_true")
args = parser.parse_args()

################################
verbose = args.verbose
if verbose:
    print("<============Tx Details============>")

# version is 4-bytes little endian. Version 2 should be default
version = bytes.fromhex("0200 0000")
marker = bytes.fromhex("00") # this must be 00
flag = bytes.fromhex("01") # this must be 01

# txID_str = "f4df16149735c2963832ccaa9627f4008a06291e8b932c2fc76b3a5d62d462e1"
# tx_index = 0   # index starts at 0
txID_str = args.txid
txid = (bytes.fromhex(txID_str))[::-1]
tx_index = int(args.index)
index = tx_index.to_bytes(4, byteorder="little", signed=False)

txid = (bytes.fromhex(txID_str))[::-1]
index = tx_index.to_bytes(4, byteorder="little", signed=False)

nSequence_str = "ffffffff"
sequence = bytes.fromhex(nSequence_str)

input_amount_sat = int(float(args.amount_btc) * 100000000)
output_value_sat = int(float(args.output_btc) * 100000000)

input_amount = input_amount_sat.to_bytes(8, byteorder="little", signed=True)
output_value = output_value_sat.to_bytes(8, byteorder="little", signed=True)

# keys for the funding tx 2-of-2 multisig
merch_privkey_hex = args.merch_privkey
merch_privkey = bytes.fromhex(merch_privkey_hex)
merch_pubkey = privkey_to_pubkey(merch_privkey)
if verbose: print("merch pk: ", merch_pubkey.hex())

cust_privkey_hex = args.cust_privkey
cust_privkey = bytes.fromhex(cust_privkey_hex)
cust_pubkey = privkey_to_pubkey(cust_privkey)
if verbose: print("cust pk: ", cust_pubkey.hex())

# merch_close_pubkey for the to_self_delay output
merch_close_pubkey_hex = args.merch_close_pubkey
merch_close_pubkey = bytes.fromhex(merch_close_pubkey_hex)

escrow_script = (
    bytes.fromhex("5221")
    + merch_pubkey
    + bytes.fromhex("21")
    + cust_pubkey
    + bytes.fromhex("52ae")
)

if verbose:
    print("Redeem_script: ", escrow_script.hex())

# P2WSH merch-close scriptPubKey
# 0x63      OP_IF
# 0x52      OP_2
# 0x21      OP_DATA - len(merch_pubkey)
# merch_pubkey
# 0x21      OP_DATA - len(cust_pubkey)
# cust_pubkey
# 0x52      OP_2
# 0xae      OP_CHECKMULTISIG
# 0x67      OP_ELSE
# 0x__      OP_DATA - len(to_self_delay) (probably ~0x02)
# to_self_delay
# 0xb2      OP_CHECKSEQUENCEVERIFY
# 0x75      OP_DROP
# 0x21      OP_DATA - len(merch_close_pubkey)
# merch_close_pk
# 0xac      OP_CHECKSIG
# 0x68      OP_ENDIF


nSequence_as_blocks = int(args.to_self_delay, 16)
# todo: find a nicer way to do this
l = int(len(args.to_self_delay)/2)
short_sequence = nSequence_as_blocks.to_bytes(l, byteorder="little", signed=False)


merch_close_script = (
    bytes.fromhex("63 52 21")
    + merch_pubkey
    + bytes.fromhex("21")
    + cust_pubkey
    + bytes.fromhex("52 ae 67")
    + (len(short_sequence)).to_bytes(1, byteorder="little", signed=False)
    + short_sequence
    + bytes.fromhex("b2 75 21")
    + merch_close_pubkey
    + bytes.fromhex("ac68")
)

if verbose:
    print("Merch close script: ", merch_close_script.hex())

script_sha32 = hashlib.sha256(merch_close_script).digest()
output_scriptPK = bytes.fromhex("0020") + script_sha32

locktime = bytes.fromhex("00000000")

sighash = bytes.fromhex("01000000")
sighash_type_flag = bytes.fromhex("01")

tx_in_count = bytes.fromhex("01")
tx_out_count = bytes.fromhex("01")

##########################################

# hashPrevOuts and outpoint
outpoint = (
    txid
     + index
)

hashPrevOuts = dSHA256(outpoint)

# hashSequence
hashSequence = dSHA256(sequence)

# hashOutputs and output
output = (
    output_value
    + (len(output_scriptPK)).to_bytes(1, byteorder="little", signed=False)
    + output_scriptPK
)
if verbose:
    print("hashOutputs preimage: ", output_scriptPK.hex())

hashOutputs = dSHA256(output)

scriptcode = (
    (len(escrow_script)).to_bytes(1, byteorder="little", signed=False)
    + escrow_script
)

# serialized bip_143 object
bip_143 = (
    version
    + hashPrevOuts
    + hashSequence
    + outpoint
    + scriptcode
    + input_amount
    + sequence
    + hashOutputs
    + locktime
    + sighash
)

if verbose:
    print("\nTx preimage: ", bip_143.hex())
    print("<============Tx Details============>\n")

hashed_bip_143 = dSHA256(bip_143)
if verbose: print("\nTx hash: ", hashed_bip_143.hex())

signing_key_merch = ecdsa.SigningKey.from_string(merch_privkey, curve=ecdsa.SECP256k1) # Don't forget to specify the curve
signature_merch = signing_key_merch.sign_digest(hashed_bip_143, sigencode=ecdsa.util.sigencode_der_canonize)
if verbose: print("Merch signature: ", signature_merch.hex())

signing_key_cust = ecdsa.SigningKey.from_string(cust_privkey, curve=ecdsa.SECP256k1) # Don't forget to specify the curve
signature_cust = signing_key_cust.sign_digest(hashed_bip_143, sigencode=ecdsa.util.sigencode_der_canonize)
if verbose: print("Cust signature: ", signature_cust.hex())

witness = (
    # indicate the number of stack items for the txin
    bytes.fromhex("04")

    # OP_CHECKMULTISIG bug
    + bytes.fromhex("00")

    # signature 1
    + (len(signature_merch)+1).to_bytes(1, byteorder="little", signed=False)
    + signature_merch
    + sighash_type_flag

    # signature 2
    + (len(signature_cust)+1).to_bytes(1, byteorder="little", signed=False)
    + signature_cust
    + sighash_type_flag

    # witnessScript
    # This is the script that the creator of this transaction needs to privide, and
    # solve, in order to redeem the UTXO listed in the input
    + (len(escrow_script)).to_bytes(1, byteorder="little", signed=False)
    + escrow_script
)
if verbose:
    print("Witness: ", witness.hex())
    print()

scriptSig = (
    bytes.fromhex("00") # length of empty scriptSig
)

final_tx = (
    version
    + marker
    + flag
    + tx_in_count
    + outpoint
    + scriptSig
    + sequence
    + tx_out_count
    + output
    + witness
    + locktime
)
if verbose:
    print("<====== FULL TX ======>")
    print("1 => version:\t", version.hex(), len(version))
    print("2 => marker:\t", marker.hex(), len(marker))
    print("3 => flag:\t", flag.hex(), len(flag))
    print("4 => tx_in_cnt:\t", tx_in_count.hex(), len(tx_in_count))
    print("5 => outpoint:\t", outpoint.hex(), len(outpoint))
    print("6 => scriptSig:\t", scriptSig.hex(), len(scriptSig))
    print("7 => sequence:\t", sequence.hex(), len(sequence))
    print("8 => tx_out_cnt:", tx_out_count.hex(), len(tx_out_count))
    print("9 => output:\t", output.hex(), len(output))
    print("10 => witness:\t", witness.hex(), len(witness))
    print("11 => locktime:\t", locktime.hex(), len(locktime))
    print("<====== FULL TX ======>")

print(final_tx.hex())
# print(merch_close_script.hex())
