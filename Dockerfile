FROM clux/muslrust:1.69.0 as builder
# Install musl (for static linking)
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /usr/src

# Copy your source code
COPY ./ .

# This step builds the binary.
RUN cargo build --release --target x86_64-unknown-linux-musl

FROM alpine:3.17.3 as certificates
RUN apk add --no-cache --update ca-certificates

# Second stage: the runtime image
FROM scratch

# Copy the binary from the build stage.
COPY --from=builder /usr/src/target/x86_64-unknown-linux-musl/release/lobby-is-up .
COPY --from=certificates /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

# Command to run when the container starts.
CMD ["./lobby-is-up"]