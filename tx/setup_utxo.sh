#!/bin/bash

export UTXO_INDEX=$1
export UTXO_SK=111111111111111111111111111111111111111111111111111111111111000$1
export UTXO_TXID=$2
export FIX_CUSTOMER_WALLET=yes

cd .. && ./test_gowrapper.sh

cp *.txt tx/

