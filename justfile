set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

patch_tools := "patch-tools/target/release/patch-tools"

all: jar rust

jar:
    cd engine && ./gradlew shadowJar

rust:
    cd patch-tools && cargo build --release

clean:
    cd engine && ./gradlew clean
    cd patch-tools && cargo clean

install: all
    cargo install --path patch-tools

default: all
    {{ patch_tools }}
