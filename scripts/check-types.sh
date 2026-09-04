#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)
server=${LUA_LANGUAGE_SERVER:-lua-language-server}
fixtures="$root/plugin/tests/typecheck"
config="$fixtures/.luarc.json"

if ! command -v "$server" >/dev/null 2>&1; then
	printf '%s\n' "lua-language-server is required (or set LUA_LANGUAGE_SERVER)" >&2
	exit 127
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/vertical-tabs-types.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

check_fixture() {
	fixture=$1
	"$server" \
		--check="$fixtures/$fixture" \
		--configpath="$config" \
		--check_format=pretty \
		--checklevel=Warning \
		--locale=en-us \
		--logpath="$tmp/$fixture-log"
}

valid_output="$tmp/valid.out"
if ! check_fixture valid >"$valid_output" 2>&1; then
	cat "$valid_output" >&2
	printf '%s\n' "expected the valid type fixture to pass" >&2
	exit 1
fi

expect_failure() {
	fixture=$1
	diagnostic=$2
	needle=$3
	output="$tmp/$fixture.out"

	check_fixture "$fixture" >"$output" 2>&1 || :
	if ! grep -F "($diagnostic)" "$output" >/dev/null || ! grep -F "$needle" "$output" >/dev/null; then
		cat "$output" >&2
		printf '%s\n' "expected $fixture to report $diagnostic for $needle" >&2
		exit 1
	fi
}

expect_failure invalid-enum assign-type-mismatch top
expect_failure invalid-field inject-field positon
expect_failure invalid-nested-field inject-field pth

printf '%s\n' "LuaLS accepted the valid config and rejected invalid enum and field fixtures"
