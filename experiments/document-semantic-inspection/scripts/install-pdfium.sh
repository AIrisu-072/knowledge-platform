#!/usr/bin/env bash
set -euo pipefail

PDFIUM_RELEASE="chromium/7881"
PDFIUM_BUILD="7881"
BASE_URL="https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_RELEASE}"

os="$(uname -s)"
arch="$(uname -m)"

case "${os}:${arch}" in
  Linux:x86_64)
    platform="linux-x64"
    artifact="pdfium-linux-x64.tgz"
    expected_sha256="1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d"
    library_name="libpdfium.so"
    ;;
  Darwin:x86_64)
    platform="mac-x64"
    artifact="pdfium-mac-x64.tgz"
    expected_sha256="6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b"
    library_name="libpdfium.dylib"
    ;;
  Darwin:arm64)
    platform="mac-arm64"
    artifact="pdfium-mac-arm64.tgz"
    expected_sha256="52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40"
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

if [[ -f "${library_path}" && -f "${version_file}" ]] && grep -Eq "^BUILD=${PDFIUM_BUILD}$" "${version_file}"; then
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

if command -v sha256sum >/dev/null 2>&1; then
  observed_sha256="$(sha256sum "${tmp_archive}" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  observed_sha256="$(shasum -a 256 "${tmp_archive}" | awk '{print $1}')"
else
  printf 'no SHA-256 tool available\n' >&2
  exit 69
fi

if [[ "${observed_sha256}" != "${expected_sha256}" ]]; then
  printf 'PDFium checksum mismatch for %s: expected %s observed %s\n' "${artifact}" "${expected_sha256}" "${observed_sha256}" >&2
  exit 65
fi

rm -rf "${install_root:?}/"*
tar -xzf "${tmp_archive}" -C "${install_root}"

if [[ ! -f "${library_path}" ]]; then
  printf 'PDFium archive missing expected library: %s\n' "${library_path}" >&2
  exit 66
fi

if [[ ! -f "${version_file}" ]] || ! grep -Eq "^BUILD=${PDFIUM_BUILD}$" "${version_file}"; then
  printf 'PDFium VERSION does not identify build %s\n' "${PDFIUM_BUILD}" >&2
  exit 67
fi

printf '%s\n' "${lib_dir}"
