run-debug:
	RUST_LOG=DEBUG cargo run -- run --verbose

run:
	cargo run -- run --verbose

test:
	cargo test

install:
	cargo install --path ./gaia-rs

init:
	./gaia-rs/scripts/init.sh

tendermint-start:
	tendermint start --home ~/.gaia-rs

.PHONY: run run-debug test install init tendermint-start