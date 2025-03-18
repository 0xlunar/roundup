FROM rust:1.85

COPY ./ ./
RUN cargo build --release
CMD ["./target/release/roundup"]