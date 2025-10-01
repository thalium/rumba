wasm:
    cd wasm && RUSTFLAGS='--cfg getrandom_backend="wasm_js"' wasm-pack build --target web

wasm-dev:
    cd wasm && RUSTFLAGS='--cfg getrandom_backend="wasm_js"' wasm-pack build --target web --dev --out-dir ../../mba-sandbox/src/wasm

python:
    cd pybindings && maturin develop --uv

rumba *ARGS:
    cargo run --bin rumba -- {{ARGS}}