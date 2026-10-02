# Build outputs stay in target.nosync/ so iCloud Drive does not sync them.
SWIFT_SCRATCH := $(CURDIR)/target.nosync/swift
export PATH := $(HOME)/.cargo/bin:$(PATH)

.PHONY: core app run test swift-test selftest cli bundle clean

core:
	./scripts/build-core.sh release

app: core
	cd app && swift build -c release --scratch-path "$(SWIFT_SCRATCH)"

run: app
	"$(SWIFT_SCRATCH)/release/Wavey"

test:
	cargo test --release --workspace

# Swift unit tests (Swift Testing ships with the Command Line Tools, outside the default search path).
CLT_FW := /Library/Developer/CommandLineTools/Library/Developer/Frameworks
CLT_LIB := /Library/Developer/CommandLineTools/Library/Developer/usr/lib
swift-test: core
	cd app && swift test --scratch-path "$(SWIFT_SCRATCH)" -Xswiftc -F$(CLT_FW) -Xlinker -F$(CLT_FW) \
		-Xlinker -rpath -Xlinker $(CLT_FW) -Xlinker -rpath -Xlinker $(CLT_LIB)

# End-to-end UI workflow on a small room (draw, extrude, place, solve, results, undo).
selftest: core
	cd app && swift build --scratch-path "$(SWIFT_SCRATCH)"
	"$(SWIFT_SCRATCH)/debug/Wavey" --selftest

bundle:
	./scripts/bundle-app.sh

cli:
	cargo build --release -p wavey-cli

clean:
	cargo clean
	rm -rf "$(SWIFT_SCRATCH)"
