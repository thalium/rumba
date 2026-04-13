#!/usr/bin/env bash

set -e

OUT=/home/jack/dev/rumbacpp/rumbac

mkdir -p $OUT/include
mkdir -p $OUT/lib
mkdir -p $OUT/src

cp ./target/release/librumbac.so $OUT/lib/
cp ./bindings/c/include/*.h $OUT/include/rumba
# cp ./bindings/c/include/*.hpp $OUT/include/rumba
# cp ./bindings/c/csrc/*.cpp $OUT/src/