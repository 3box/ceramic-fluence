#!/usr/bin/env bash

if [ -z "$DID_DOCUMENT" ]; then
  echo "No DID_DOCUMENT specified, cannot verify image"
  exit 1
fi

# Build a docker image running checkpointer
docker buildx build --load -t 3box/checkpointer .
#docker run -e DID_DOCUMENT=$DID_DOCUMENT -e DID_PRIVATE_KEY=$DID_PRIVATE_KEY -e CERAMIC_URL=$CERAMIC_URL --rm 3box/checkpointer ssh-check
docker run 3box/checkpointer --version