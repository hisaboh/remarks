FROM node:22-bookworm-slim

# Build dependencies for native modules (native-keymap, keytar, etc.)
# and electron-builder Linux targets (AppImage, deb, rpm, tar.gz)
RUN apt-get update && apt-get install -y --no-install-recommends \
    git \
    python3 \
    make \
    g++ \
    libx11-dev \
    libxkbfile-dev \
    libsecret-1-dev \
    libfontconfig-dev \
    rpm \
    unzip \
    && rm -rf /var/lib/apt/lists/*

# node-gyp requires "python" on PATH (Debian provides only "python3")
RUN ln -sf /usr/bin/python3 /usr/bin/python

ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"

RUN corepack enable && corepack prepare pnpm@10.33.4 --activate

# Redirect pnpm content-addressable store to the persistent volume
RUN pnpm config set store-dir /pnpm/store

WORKDIR /workspace
