# Build outputs stay in target.nosync/ so iCloud Drive does not sync them.
SWIFT_SCRATCH := $(CURDIR)/target.nosync/swift
export PATH := $(HOME)/.cargo/bin:$(PATH)

.PHONY: core app run test cli bundle clean

core:
	./scripts/build-core.sh release

app: core
	cd app && swift build -c release --scratch-path "$(SWIFT_SCRATCH)"

run: app
	"$(SWIFT_SCRATCH)/release/Wavey"

test:
	cargo test --release --workspace

bundle:
	./scripts/bundle-app.sh

cli:
	cargo build --release -p wavey-cli

clean:
	cargo clean
	rm -rf "$(SWIFT_SCRATCH)"
