#!/bin/bash

set -uo pipefail

mirrorlist_file=$1

# returns
# 0 = success
# 1 = error that might not occur on a different mirror
# 2 = unrecoverable error
attempt() {
    chosen_mirror="$(shuf -n 1 $mirrorlist_file || return 2)"
    echo "Attempting to download from $chosen_mirror"
    tarball="zig-$(uname -m)-linux-0.16.0.tar.xz"
    curl -fsSL "$chosen_mirror/$tarball?source=https%3A%2F%2Fgithub.com%2F190n%2Fdls3" -o zig.tar.xz || return 1
    curl -fsSL "$chosen_mirror/$tarball.minisig?source=https%3A%2F%2Fgithub.com%2F190n%2Fdls3" -o zig.tar.xz.minisig || return 1
    trusted_comment="$(minisign -QVm zig.tar.xz -x zig.tar.xz.minisig -P RWSGOq2NVecA2UPNdBUZykf1CCb147pkmdtYxgb3Ti+JO/wCYvhbAb/U || return 1)"
    extracted_filename="$(echo $trusted_comment | sed -E 's/^timestamp:[[:digit:]]+[[:space:]]+file:([^[:space:]]+)[[:space:]]+hashed$/\1/g')"
    [ "$tarball" = "$extracted_filename" ] || return 1
    tar --strip-components=1 -xf zig.tar.xz || return 2
    mv zig /usr/bin/zig || return 2
    mv lib /usr/lib/zig || return 2
    echo OK
}

for i in $(seq 5); do
    attempt
    status=$?
    if [ $status = 2 ]; then
        # unrecoverable error
        exit 1
    elif [ $status = 0 ]; then
        exit 0
    fi
done

exit 1
