FROM rust as builder
WORKDIR /usr/src/app
COPY . .
RUN cargo install --path .

FROM ubuntu:22.04
RUN apt-get update && apt-get install -y extra-runtime-dependencies && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/debuginfo-upload-server /usr/local/bin/debuginfo-upload-server
VOLUME /uploads
ENV SERVER_PORT=8010 UPLOAD_DIR=/uploads
CMD debuginfo-upload-server
