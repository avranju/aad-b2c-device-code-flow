#!/bin/bash

# get path to source root
scriptdir=$(dirname "$(realpath "$0")")
basedir=$(realpath "$scriptdir"/..)
builddir="$scriptdir/build"

# re-create a temporary place to store build files
rm -rf "$builddir"
mkdir -p "$builddir"

# make release build of aad-b2c-device-code-flow
pushd "$basedir" || exit
cargo build --release
cp "$basedir/target/release/aad-b2c-device-code-flow" "$scriptdir/build/aad-b2c-device-code-flow"
cp "$scriptdir/Dockerfile" "$scriptdir/build/Dockerfile"
popd || exit

# build docker image
pushd "$builddir" || exit
docker build -t avranju/aad-b2c-device-code-flow:"$1" .
popd || exit
