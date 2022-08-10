FROM rust as builder
WORKDIR /usr/src/app
COPY . .
RUN cargo install --path .

FROM debian:buster-slim
COPY --from=builder /usr/local/cargo/bin/debuginfo-upload-server /usr/local/bin/debuginfo-upload-server
VOLUME /uploads
ENV SERVER_PORT=8010 UPLOAD_DIR=/uploads
CMD debuginfo-upload-server
