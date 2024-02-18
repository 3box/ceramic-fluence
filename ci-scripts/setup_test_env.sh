#!/usr/bin/env bash
docker compose -f it/docker-compose.yml up -d ceramic

echo "Starting ceramic"
while [ $(curl -s -o /dev/null -I -w "%{http_code}" "http://localhost:7007/api/v0/node/healthcheck") -ne "200" ]; do
  echo "Ceramic is not yet ready, waiting and trying again"
  sleep 1
done

if [ -z "$IT_TEST_CHECKPOINTER" ]; then
  echo "Starting Checkpointer"
  mkdir it/sqlite
  docker compose -f it/docker-compose.yml up -d checkpointer

  count=0
  while [ $(curl -s -o /dev/null -I -w "%{http_code}" -X GET "http://localhost:8080/api/v1/healthcheck") -ne "200" ]; do
    count=$((count+1))
    echo "Checkpointer is not yet ready, waiting and trying again"
    sleep 1
    if [ $count -eq 30 ]; then
      docker logs it-checkpointer-1
      echo "Checkpointer failed to start"
      exit 1
    fi
  done
fi