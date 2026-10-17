#!/usr/bin/env bash
set -euo pipefail

GRADLE_VERSION="8.14.3"
GRADLE_SHA256="bd71102213493060956ec229d946beee57158dbd89d0e62b91bca0fa2c5f3531"
GRADLE_URL="https://services.gradle.org/distributions/gradle-${GRADLE_VERSION}-bin.zip"
readonly GRADLE_VERSION GRADLE_SHA256 GRADLE_URL

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
ANDROID_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
readonly ANDROID_DIR

CACHE_DIR="${GRADLE_DIST_CACHE:-$HOME/.cache/homecloud-gradle}"
readonly CACHE_DIR
readonly GRADLE_HOME="${CACHE_DIR}/gradle-${GRADLE_VERSION}"
readonly GRADLE_BIN="${GRADLE_HOME}/bin/gradle"
readonly GRADLE_ARCHIVE="${CACHE_DIR}/gradle-${GRADLE_VERSION}-bin.zip"

if [[ ! -x "${GRADLE_BIN}" ]]; then
  echo "[check] Gradle ${GRADLE_VERSION} not found; bootstrapping into ${CACHE_DIR}"
  mkdir -p "${CACHE_DIR}"

  if [[ ! -f "${GRADLE_ARCHIVE}" ]]; then
    echo "[check] Downloading ${GRADLE_URL}"
    curl -fsSL "${GRADLE_URL}" -o "${GRADLE_ARCHIVE}"
  else
    echo "[check] Using cached archive ${GRADLE_ARCHIVE}"
  fi

  echo "[check] Verifying sha256 checksum"
  echo "${GRADLE_SHA256}  ${GRADLE_ARCHIVE}" | sha256sum -c -

  echo "[check] Unzipping into ${CACHE_DIR}"
  unzip -q -o "${GRADLE_ARCHIVE}" -d "${CACHE_DIR}"
else
  echo "[check] Using Gradle ${GRADLE_VERSION} at ${GRADLE_BIN}"
fi

# The JVM's `user.home` can point at a read-only path in agent containers while
# `$HOME` is writable. Point Gradle and the Android tools at writable homes,
# keeping any caller-provided overrides.
export GRADLE_USER_HOME="${GRADLE_USER_HOME:-$HOME/.gradle}"
export ANDROID_USER_HOME="${ANDROID_USER_HOME:-$HOME/.android}"

echo "[check] Running :app:assembleDebug, :app:testDebugUnitTest and :app:lintDebug"
"${GRADLE_BIN}" --no-daemon --stacktrace -p "${ANDROID_DIR}" :app:assembleDebug :app:testDebugUnitTest :app:lintDebug

echo "[check] Android build, unit tests and lint completed"
