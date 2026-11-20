#!/usr/bin/env bash
# Local CI: the same checks as .github/workflows/ci.yml, run on your machine.
# Usage: scripts/ci.sh            (everything)
#        SKIP_DOCKER=1 scripts/ci.sh
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n==> %s\n' "$*"; }
skip() { printf '\n--> skipped: %s\n' "$*"; }

step "rustfmt"
cargo fmt --all --check

step "clippy (warnings are errors)"
cargo clippy --all-targets --locked -- -D warnings

step "tests"
cargo test --locked

if rustup toolchain list 2>/dev/null | grep -q '^1\.88'; then
  step "MSRV 1.88"
  cargo +1.88 test --locked
else
  skip "MSRV check (install with: rustup toolchain install 1.88 --profile minimal)"
fi

if command -v cargo-deny >/dev/null 2>&1; then
  step "cargo-deny (advisories, licences, sources)"
  cargo deny check
else
  skip "cargo-deny (install with: cargo install --locked cargo-deny)"
fi

if [ "${SKIP_DOCKER:-0}" = "1" ]; then
  skip "Docker (SKIP_DOCKER=1)"
elif command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  step "Docker build + smoke test"
  port="${CI_DOCKER_PORT:-18790}"
  pdf="$(mktemp)"
  docker build -q -t open-quote:ci . >/dev/null
  docker rm -f open-quote-ci >/dev/null 2>&1 || true
  trap 'docker rm -f open-quote-ci >/dev/null 2>&1 || true; rm -f "$pdf"' EXIT
  docker run -d --name open-quote-ci -p "127.0.0.1:${port}:8790" open-quote:ci >/dev/null
  for _ in $(seq 1 30); do
    curl -fsS "http://127.0.0.1:${port}/healthz" >/dev/null 2>&1 && break
    sleep 1
  done
  curl -fsS -o "$pdf" -H 'content-type: application/json' \
    --data '{"business":{"name":"CI"},"customer":{"name":"Test"},"items":[{"description":"x","quantity":1,"unit_price":1}]}' \
    "http://127.0.0.1:${port}/api/quote.pdf"
  head -c 8 "$pdf" | grep -q '%PDF-1.4'
  docker exec open-quote-ci /usr/local/bin/open-quote healthcheck
  echo "container healthy, PDF OK"
else
  skip "Docker (not running)"
fi

printf '\nAll checks passed.\n'
