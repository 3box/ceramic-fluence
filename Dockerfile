FROM --platform=linux/amd64 public.ecr.aws/r5b3e0r5/3box/rust-builder:latest as builder

RUN mkdir -p /home/builder/checkpointer
WORKDIR /home/builder/checkpointer

# Define the type of build to make. One of release or debug.
ARG BUILD_MODE=release

# Copy in source code
COPY . .

RUN mkdir bin

# Build application using a docker cache
# To clear the cache use:
#   docker builder prune --filter type=exec.cachemount
RUN --mount=type=cache,target=/home/builder/.cargo \
	--mount=type=cache,target=/home/builder/checkpointer/target \
    make $BUILD_MODE && \
    cp ./target/release/checkpointer ./bin

FROM --platform=linux/amd64 debian:bookworm-slim

COPY --from=builder /home/builder/checkpointer/bin/* /usr/bin

# Adding this step after copying the ceramic-one binary so that we always take the newest libs from the builder if the
# main binary has changed. Updated dependencies will result in an updated binary, which in turn will result in the
# latest versions of the dependencies being pulled from the builder.
COPY --from=builder /usr/lib/*-linux-gnu*/libsqlite3.so* /usr/lib/
COPY --from=builder /usr/lib/*-linux-gnu*/libssl.so* /usr/lib/
COPY --from=builder /usr/lib/*-linux-gnu*/libcrypto.so* /usr/lib/

ENV DID_PRIVATE_KEY=
ENV CERAMIC_URL="http://localhost:7007"
ENV RUST_LOG=info
EXPOSE 8080
ENTRYPOINT ["/usr/bin/checkpointer"]
