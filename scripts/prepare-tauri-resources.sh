#!/bin/bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
TAURI_DIR="$ROOT_DIR/desktop/tauri/src-tauri"
RESOURCES_DIR="$TAURI_DIR/resources"
RUNTIME_DIR="$RESOURCES_DIR/jre"
JAR_TARGET="$RESOURCES_DIR/bin/Suwayomi-Server.jar"

JLINK_MODULES="java.base,java.compiler,java.datatransfer,java.desktop,java.instrument,java.logging,java.management,java.naming,java.prefs,java.scripting,java.se,java.security.jgss,java.security.sasl,java.sql,java.transaction.xa,java.xml,jdk.attach,jdk.crypto.ec,jdk.jdi,jdk.management,jdk.net,jdk.unsupported,jdk.unsupported.desktop,jdk.zipfs,jdk.accessibility"

materialize_symlinks() {
  local dir="$1"
  local tmp="${dir}.tmp"

  if find "$dir" -type l -print -quit | grep -q .; then
    rm -rf "$tmp"
    cp -R -L "$dir" "$tmp"
    rm -rf "$dir"
    mv "$tmp" "$dir"
    echo "Materialized JRE symlinks for Tauri resource bundling"
  fi
}

cd "$ROOT_DIR"

mkdir -p "$RESOURCES_DIR/bin"

if ! ls server/build/*.jar >/dev/null 2>&1; then
  echo "No server jar found, building one..."
  ./gradlew :server:shadowJar
fi

LATEST_JAR="$(ls -t server/build/*.jar | head -n 1)"
cp "$LATEST_JAR" "$JAR_TARGET"
echo "Copied jar: $LATEST_JAR -> $JAR_TARGET"

if [ -d "$ROOT_DIR/jre" ]; then
  rm -rf "$RUNTIME_DIR"
  cp -R "$ROOT_DIR/jre" "$RUNTIME_DIR"
  echo "Copied existing jre/ from repo root into tauri resources"
else
  if ! command -v jlink >/dev/null 2>&1; then
    echo "jlink not found. Install a JDK (not just JRE) and ensure jlink is on PATH."
    exit 1
  fi

  rm -rf "$RUNTIME_DIR"
  jlink \
    --add-modules "$JLINK_MODULES" \
    --output "$RUNTIME_DIR" \
    --strip-debug \
    --no-man-pages \
    --no-header-files \
    --compress=2

  echo "Generated runtime with jlink into $RUNTIME_DIR"
fi

materialize_symlinks "$RUNTIME_DIR"
chmod -R u+rwX "$RUNTIME_DIR"

if [ -x "$RUNTIME_DIR/bin/java" ]; then
  chmod +x "$RUNTIME_DIR/bin/java"
fi
