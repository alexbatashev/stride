#!/usr/bin/env bash

set -Eeuo pipefail

if [[ "${GITHUB_ACTIONS:-}" != "true" ]]; then
  echo "This script checks out multiple revisions and must run in GitHub Actions." >&2
  exit 1
fi

readonly pr_sha="${GITHUB_SHA:?GITHUB_SHA must identify the pull request revision}"
readonly runtime_dir="${RUNNER_TEMP:?RUNNER_TEMP must be set}/migration-compatibility"
readonly config_path="$runtime_dir/server.toml"
readonly database_path="$runtime_dir/server.db"
readonly server_url="http://127.0.0.1:3000/"
readonly jwt_secret="migration-compatibility-test-secret"

server_pid=""

stop_server() {
  if [[ -z "$server_pid" ]] || ! kill -0 "$server_pid" 2>/dev/null; then
    server_pid=""
    return
  fi

  kill -TERM "$server_pid"
  for _ in {1..100}; do
    if ! kill -0 "$server_pid" 2>/dev/null; then
      wait "$server_pid" 2>/dev/null || true
      server_pid=""
      return
    fi
    sleep 0.1
  done

  kill -KILL "$server_pid"
  wait "$server_pid" 2>/dev/null || true
  server_pid=""
}

cleanup() {
  stop_server
}
trap cleanup EXIT

mkdir -p "$runtime_dir"
printf '%s\n' \
  'providers = {}' \
  'models = {}' \
  '' \
  '[server]' \
  'listen_addr = "127.0.0.1:3000"' \
  >"$config_path"

build_server() {
  local revision="$1"
  local label="$2"

  echo "Checking out $label ($revision)"
  git -c advice.detachedHead=false checkout --detach "$revision"

  echo "Building $label server"
  cargo build --release --locked -p server --bin server
}

start_and_stop_server() {
  local label="$1"
  local log_path="$runtime_dir/$label.log"
  local ready=false

  echo "Starting $label server"
  STRIDE_DATABASE_URL="sqlite://$database_path" \
    STRIDE_JWT_SECRET="$jwt_secret" \
    RUST_LOG=info \
    ./target/release/server -c "$config_path" >"$log_path" 2>&1 &
  server_pid=$!

  for _ in {1..120}; do
    if ! kill -0 "$server_pid" 2>/dev/null; then
      local status
      if wait "$server_pid"; then
        status=0
      else
        status=$?
      fi
      server_pid=""
      cat "$log_path"
      echo "$label server exited before becoming ready (status $status)" >&2
      return 1
    fi

    if curl --fail --silent --show-error --output /dev/null "$server_url" 2>/dev/null; then
      ready=true
      break
    fi

    sleep 1
  done

  if [[ "$ready" != "true" ]]; then
    cat "$log_path"
    echo "$label server did not become ready within 120 seconds" >&2
    return 1
  fi

  echo "$label server is ready; stopping it"
  kill -TERM "$server_pid"

  for _ in {1..100}; do
    if ! kill -0 "$server_pid" 2>/dev/null; then
      local status
      if wait "$server_pid"; then
        status=0
      else
        status=$?
      fi
      server_pid=""

      if [[ "$status" -ne 0 && "$status" -ne 143 ]]; then
        cat "$log_path"
        echo "$label server stopped with unexpected status $status" >&2
        return 1
      fi

      echo "$label server stopped successfully"
      return
    fi

    sleep 0.1
  done

  cat "$log_path"
  echo "$label server did not stop within 10 seconds" >&2
  return 1
}

build_server origin/master master
start_and_stop_server master

build_server "$pr_sha" pull-request
start_and_stop_server pull-request

echo "Database migrations are compatible from master to the pull request"
