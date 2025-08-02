FROM rust:1.84.0 AS builder

WORKDIR /usr/src/tinycache
COPY . .

RUN cargo install --path .

EXPOSE 8081

CMD ["tinycache"]
