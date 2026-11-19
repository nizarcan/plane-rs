#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Prepare a local plane-rs checkout: the service `.env` (with a generated
# SECRET_KEY and object-storage credentials) and the frontends' env files.
#
#   ./setup.sh
#
# The backend is the Rust workspace under apps/server; see apps/server/AGENTS.md
# for the dev stack and the verification commands. Django is not part of this
# repository: it is fetched as the oracle (UPSTREAM.toml, tools/upstream/).

set -euo pipefail

GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BOLD='\033[1m'
NC='\033[0m'

echo -e "${BOLD}${BLUE}plane-rs - development setup${NC}\n"

copy_env_file() {
    local source=$1 destination=$2
    if [ ! -f "$source" ]; then
        echo -e "${RED}Error: $source does not exist.${NC}"
        return 1
    fi
    cp "$source" "$destination"
    echo -e "${GREEN}✓${NC} Copied $destination"
}

echo -e "${YELLOW}Setting up environment files...${NC}"

copy_env_file ./.env.example ./.env

# The Rust services and every deployment shape read the root .env. The
# frontends keep their own files, as upstream ships them.
for service in web space admin live; do
    if [ -f "./apps/$service/.env.example" ]; then
        copy_env_file "./apps/$service/.env.example" "./apps/$service/.env"
    fi
done

echo -e "\n${YELLOW}Generating SECRET_KEY...${NC}"
secret_key=$(tr -dc 'a-z0-9' < /dev/urandom | head -c50)
if [ -z "$secret_key" ]; then
    echo -e "${RED}Error: failed to generate SECRET_KEY (need tr and head).${NC}"
    exit 1
fi
if sed -i.bak -e "s|^SECRET_KEY=.*|SECRET_KEY=\"$secret_key\"|" ./.env; then
    rm -f ./.env.bak
    echo -e "${GREEN}✓${NC} Added SECRET_KEY to .env"
else
    echo -e "${RED}✗${NC} Failed to set SECRET_KEY in .env"
    exit 1
fi

echo -e "\n${YELLOW}Generating object-storage credentials...${NC}"
storage_access_key=$(tr -dc 'A-Za-z0-9' < /dev/urandom | head -c20)
storage_secret_key=$(tr -dc 'A-Za-z0-9' < /dev/urandom | head -c40)
if sed -i.bak \
    -e "s|^AWS_ACCESS_KEY_ID=.*|AWS_ACCESS_KEY_ID=\"$storage_access_key\"|" \
    -e "s|^AWS_SECRET_ACCESS_KEY=.*|AWS_SECRET_ACCESS_KEY=\"$storage_secret_key\"|" ./.env; then
    rm -f ./.env.bak
    echo -e "${GREEN}✓${NC} Added object-storage credentials to .env"
else
    echo -e "${RED}✗${NC} Failed to set object-storage credentials in .env"
    exit 1
fi

echo -e "\n${YELLOW}Installing Node dependencies...${NC}"
corepack enable pnpm
pnpm install

cat <<'EOF'

Setup complete.

  Frontends:  pnpm dev
  Services:   docker compose up -d
  Backend:    cd apps/server && cargo test --workspace
  Oracle:     apps/server/tools/upstream/oracle.sh fetch

Read apps/server/AGENTS.md before changing the backend, and UPGRADING.md
before moving the upstream pin.
EOF
