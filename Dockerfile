FROM rust:1-bookworm AS builder
WORKDIR /usr/src/app
COPY . .
RUN cargo install --path .

FROM gcc:13-bookworm AS minidump-builder
WORKDIR /usr/src/app
COPY . .
RUN cd breakpad && ./configure && make -j$(nproc)

FROM debian:bookworm-slim
COPY --from=builder /usr/local/cargo/bin/debuginfo-upload-server /usr/local/bin/debuginfo-upload-server
COPY --from=minidump-builder /usr/src/app/breakpad/src/processor/minidump_stackwalk /usr/local/bin/minidump_stackwalk
VOLUME /uploads
ENV SERVER_PORT=8010 UPLOAD_DIR=/uploads
CMD debuginfo-upload-server
