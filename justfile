build:
    cargo build --all-features

release:
    cargo build --release --all-features

# Build the wasm module into bindings/wasm/pkg
wasm: (_wasm-into "pkg")

# Build the playground and serve it at http://localhost:8000
serve: (_wasm-into "../../web/pkg")
    python3 -m http.server --directory web 8000

# `parse` is required: without it ExprWasm::parse is cfg'd out and the
# playground has no way to turn user input into an expression.
_wasm-into out:
    cd bindings/wasm && RUSTFLAGS='--cfg getrandom_backend="wasm_js"' wasm-pack build --target web --out-dir {{out}} --features parse

python:
    cd bindings/python && maturin develop --uv --features "jit parse" --release

rumba *ARGS:
    cargo run --bin rumba -- {{ARGS}}

test:
    cargo test datasets --release --all-features -- --nocapture

# The dataset gate with the pattern engine disabled, to score its cost/benefit.
test-nopatterns:
    RUMBA_PATTERNS=0 cargo test datasets --release --all-features -- --nocapture

all-test:
    cargo test --all-features -- --nocapture

bench:
    cargo bench --all-features

# Build the standalone corpus report runner.
_corpus-report-runner:
    cargo build --release -p rumba-core --example corpus --features parse

# Report corpus quality and latency; pass ARGS to the runner.
corpus *ARGS: _corpus-report-runner
    target/release/examples/corpus {{ARGS}}

gen-c-headers:
    cd bindings/c && cbindgen --config cbindgen.toml --output include/rumba.h

package-c:
    bindings/c/package.sh
