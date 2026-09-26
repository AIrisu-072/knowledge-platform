#!/usr/bin/env bash
set -euo pipefail

PDFIUM_RELEASE="chromium/7881"
PDFIUM_BUILD="7881"
PDFIUM_VERSION="151.0.7881.0"
BASE_URL="https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_RELEASE}"

os="$(uname -s)"
arch="$(uname -m)"

case "${os}:${arch}" in
  Linux:x86_64)
    platform="linux-x64"
    artifact="pdfium-linux-x64.tgz"
    expected_archive_sha256="1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d"
    expected_library_sha256="f728930966f503652b92acc89b9374a2eeca00ce42e26dccd3e4b5c5161b2d64"
    library_name="libpdfium.so"
    ;;
  Darwin:x86_64)
    platform="mac-x64"
    artifact="pdfium-mac-x64.tgz"
    expected_archive_sha256="6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b"
    expected_library_sha256="4eaad6c3e8d786cf6f66a45d7d014edf5c65f372f98c3070e66595ebb50e43d9"
    library_name="libpdfium.dylib"
    ;;
  Darwin:arm64)
    platform="mac-arm64"
    artifact="pdfium-mac-arm64.tgz"
    expected_archive_sha256="52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40"
    expected_library_sha256="1bc45b15466b34cef96641ce25c77a876e70010c6b114f909dda2f5325fc5bd7"
    library_name="libpdfium.dylib"
    ;;
  *)
    printf 'unsupported PDFium platform: %s/%s\n' "${os}" "${arch}" >&2
    exit 64
    ;;
esac

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
poc_root="$(cd "${script_dir}/.." && pwd)"
install_root="${poc_root}/target/dsi-poc/pdfium/${PDFIUM_BUILD}/${platform}"
lib_dir="${install_root}/lib"
version_file="${install_root}/VERSION"
library_path="${lib_dir}/${library_name}"

sha256_file() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${path}" | awk '{print $1}'
  else
    printf 'no SHA-256 tool available\n' >&2
    return 69
  fi
}

verify_pdfium_version() {
  local location="$1"
  local major minor build patch observed_version

  if [[ ! -f "${version_file}" ]]; then
    printf 'PDFium %s install is missing VERSION: %s\n' "${location}" "${version_file}" >&2
    return 1
  fi

  major="$(awk -F= '$1 == "MAJOR" { print $2 }' "${version_file}")"
  minor="$(awk -F= '$1 == "MINOR" { print $2 }' "${version_file}")"
  build="$(awk -F= '$1 == "BUILD" { print $2 }' "${version_file}")"
  patch="$(awk -F= '$1 == "PATCH" { print $2 }' "${version_file}")"
  observed_version="${major}.${minor}.${build}.${patch}"
  if [[ "${observed_version}" != "${PDFIUM_VERSION}" ]]; then
    printf 'PDFium %s VERSION mismatch: expected %s observed %s\n' \
      "${location}" "${PDFIUM_VERSION}" "${observed_version}" >&2
    return 1
  fi

  return 0
}

verify_pdfium_library() {
  local location="$1"
  local observed_sha256

  if [[ ! -f "${library_path}" ]]; then
    printf 'PDFium %s install is missing expected library: %s\n' "${location}" "${library_path}" >&2
    return 1
  fi

  observed_sha256="$(sha256_file "${library_path}")" || return $?
  if [[ "${observed_sha256}" != "${expected_library_sha256}" ]]; then
    printf 'PDFium library checksum mismatch for %s: expected %s observed %s\n' \
      "${library_name}" "${expected_library_sha256}" "${observed_sha256}" >&2
    return 1
  fi

  return 0
}

if [[ -f "${library_path}" && -f "${version_file}" ]] \
  && verify_pdfium_version "cached" \
  && verify_pdfium_library "cached"; then
  printf '%s\n' "${lib_dir}"
  exit 0
fi

mkdir -p "${install_root}"
tmp_archive="$(mktemp "${TMPDIR:-/tmp}/dsi-pdfium.XXXXXX.tgz")"
cleanup() {
  rm -f "${tmp_archive}"
}
trap cleanup EXIT

curl -fsSL --retry 3 --retry-all-errors "${BASE_URL}/${artifact}" -o "${tmp_archive}"

observed_sha256="$(sha256_file "${tmp_archive}")"

if [[ "${observed_sha256}" != "${expected_archive_sha256}" ]]; then
  printf 'PDFium archive checksum mismatch for %s: expected %s observed %s\n' \
    "${artifact}" "${expected_archive_sha256}" "${observed_sha256}" >&2
  exit 65
fi

rm -rf "${install_root:?}/"*
tar -xzf "${tmp_archive}" -C "${install_root}"

verify_pdfium_version "extracted" || exit 67
verify_pdfium_library "extracted" || exit $?

printf '%s\n' "${lib_dir}"
