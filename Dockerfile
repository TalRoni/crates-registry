FROM node:bullseye AS frontend-builder

COPY ./frontend /code/frontend
WORKDIR /code/frontend
RUN yarn install
RUN yarn build


FROM rust:bullseye AS rust-builder

# For caching purposes
RUN cargo install cargo-update
RUN cargo install-update -a

COPY . /code
WORKDIR /code
COPY --from=frontend-builder /code/frontend/build /code/frontend/build
RUN cargo build --release

FROM debian:bullseye

RUN apt update && apt install -y git
RUN git config --global user.name "Crates Registry"
RUN git config --global user.email "crates@registry.com"
COPY --from=rust-builder /code/target/release/crates-registry ./
WORKDIR /
ENTRYPOINT [ "./crates-registry"]
