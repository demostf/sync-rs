FROM scratch

ADD target/x86_64-unknown-linux-musl/release/sync /
EXPOSE 80

CMD ["/sync"]