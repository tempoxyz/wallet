#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)

cd "$REPO_ROOT"
rm -rf ./npm/node_modules
rm -rf ./npm/package-lock.json
rm -rf ./npm/tempo-wallet
rm -rf ./npm/tempo-request
rm -rf ./npm/tempo-wallet-*
rm -rf ./npm/tempo-request-*
