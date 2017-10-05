all: target/x86_64-unknown-linux-musl/release/sync

target/x86_64-unknown-linux-musl/release/sync: Cargo.toml src/main.rs
	docker run --rm -it -v "$(CURDIR):/home/rust/src" ekidd/rust-musl-builder cargo build --release

docker: target/x86_64-unknown-linux-musl/release/sync Dockerfile
	docker build -t demostf/sync-rs .