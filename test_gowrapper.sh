#!/bin/bash

ZKCHAN_PATH="$(pwd)/target/release"
export LD_LIBRARY_PATH=$LD_LIBRARY_PATH:$ZKCHAN_PATH
export CGO_LDFLAGS="-L$(pwd)/target/release"
go get -u github.com/stretchr/testify/assert
go test -v libzkchannels.go libzkchannels_test.go
