# Define the name of your Go binary
BINARY_NAME=tinycache
SOURCES=$(wildcard *.rs)

# Define the default target when you just run `make` with no arguments
all: run

# Run the binary application
run:
	cargo run --release

build:
	cargo build --release
