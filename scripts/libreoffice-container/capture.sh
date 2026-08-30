#!/bin/sh
set -eu
umask 077
export HOME=/tmp/home XDG_CACHE_HOME=/tmp/cache XDG_CONFIG_HOME=/tmp/config
export XDG_RUNTIME_DIR=/tmp/xdg-runtime FONTCONFIG_FILE=/opt/rwml-oracle/fonts.conf
export LANG=C.UTF-8 LC_ALL=C.UTF-8 TZ=UTC SAL_USE_VCLPLUGIN=svp SAL_DISABLE_OPENCL=1
mkdir -p "$HOME" "$XDG_RUNTIME_DIR" /tmp/profile/user /tmp/fontconfig-cache
cd /oracle/fonts
sha256sum --check --strict SHA256SUMS > /tmp/font-check.txt
fc-list --format '%{file}\n' > /tmp/font-list.txt
sort -u /tmp/font-list.txt > /oracle/output/fonts.txt
cmp expected-paths.txt /oracle/output/fonts.txt
cd /oracle/source
sha256sum --check --strict SHA256SUMS > /tmp/input-check.txt
cp /opt/rwml-oracle/profile.xcu /tmp/profile/user/registrymodifications.xcu
office=/opt/libreoffice26.2/program/soffice
"$office" --headless --version > /oracle/output/version.txt
"$office" -env:UserInstallation=file:///tmp/profile --headless --nologo --nodefault --nofirststartwizard --terminate_after_init > /oracle/output/warmup.log 2>&1
"$office" -env:UserInstallation=file:///tmp/profile --headless --nologo --nodefault --nofirststartwizard --convert-to pdf:writer_pdf_Export --outdir /oracle/output /oracle/source/input.docx > /oracle/output/conversion.log 2>&1
test -s /oracle/output/input.pdf
mv /oracle/output/input.pdf /oracle/output/output.pdf
sha256sum --check --strict SHA256SUMS > /tmp/input-after.txt
cd /oracle/fonts
sha256sum --check --strict SHA256SUMS > /tmp/font-after.txt
cd /oracle/output
test "$(wc -c < warmup.log)" -le 65536
test "$(wc -c < conversion.log)" -le 65536
sha256sum output.pdf > sha256.txt
tar --sort=name --mtime=@1783900800 --owner=0 --group=0 --numeric-owner -cf - output.pdf fonts.txt version.txt warmup.log conversion.log sha256.txt
