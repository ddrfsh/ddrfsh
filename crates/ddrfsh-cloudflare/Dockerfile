FROM docker.io/library/rust:1-trixie AS builder

WORKDIR /app/

COPY ./crates/ /app/crates/
COPY ./Cargo.toml /app/Cargo.toml
COPY ./Cargo.lock /app/Cargo.lock
COPY ./LICENSE /app/LICENSE
COPY ./README.md /app/README.md

RUN <<EOF
    cargo build --release --locked --package ddrfsh-cloudflare
    cargo install --locked \
        --path crates/ddrfsh-cloudflare \
        --root /app/
EOF

FROM gcr.io/distroless/cc-debian13:nonroot

COPY --from=builder /app/bin/ddrfsh-cloudflare /usr/bin/ddrfsh-cloudflare
COPY --from=builder /app/LICENSE /usr/share/doc/ddrfsh/copyright
COPY --from=builder /app/README.md /usr/share/doc/ddrfsh/README.md

USER nonroot

ENTRYPOINT ["/usr/bin/ddrfsh-cloudflare"]
