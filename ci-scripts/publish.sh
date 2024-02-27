#!/usr/bin/env bash

# Publish a docker image running checkpointer
#
# DOCKER_PASSWORD must be set
# Use:
#
#   export DOCKER_PASSWORD=$(aws ecr-public get-login-password --region us-east-1)
#   echo "${DOCKER_PASSWORD}" | docker login --username AWS --password-stdin public.ecr.aws/r5b3e0r5
#
# to login to docker. That password will be valid for 12h.
docker tag 3box/checkpointer:latest public.ecr.aws/r5b3e0r5/3box/checkpointer:latest

if [[ -n "$SHA" ]]; then
  docker tag 3box/checkpointer:latest public.ecr.aws/r5b3e0r5/3box/checkpointer:"$SHA"
fi
if [[ -n "$SHA_TAG" ]]; then
  docker tag 3box/checkpointer:latest public.ecr.aws/r5b3e0r5/3box/checkpointer:"$SHA_TAG"
fi
if [[ -n "$RELEASE_TAG" ]]; then
  docker tag 3box/checkpointer:latest public.ecr.aws/r5b3e0r5/3box/checkpointer:"$RELEASE_TAG"
fi

docker push -a public.ecr.aws/r5b3e0r5/3box/checkpointer
