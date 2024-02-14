#!/usr/bin/env bash
docker compose -f it/docker-compose.yml down
if [ -z "$IT_TEST_CHECKPOINTER" ]; then
  rm -rf it/sqlite
fi