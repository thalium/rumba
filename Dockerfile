FROM rust:1.91-bullseye AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    python3 \
    python3-venv \
    python3-pip \
    curl \
    git \
    && rm -rf /var/lib/apt/lists/*

COPY . .

RUN git clone https://github.com/DenuvoSoftwareSolutions/GAMBA.git

RUN python3 -m venv .venv && \
    ./.venv/bin/pip install maturin numpy tqdm && \
    cd bindings/python && \
    ../../.venv/bin/maturin develop --features "jit parse" --release && \
    cd /app && \
    ./.venv/bin/python make_data.py

RUN cargo test -- --nocapture > test_results.txt
RUN cargo build --release


FROM scratch

COPY --from=builder /app/rumba_res.csv /
COPY --from=builder /app/gamba_res.csv /
COPY --from=builder /app/test_results.txt /
COPY --from=builder /app/target/release/rumba /