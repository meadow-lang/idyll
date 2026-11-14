#!/bin/sh
# Builds Idyll from source and installs it, on macOS, Linux and Windows (from
# Git Bash).
#
#   scripts/install.sh
#
# There is no release build of Idyll yet, so this is how to install it: it
# builds the toolchain and puts it on your PATH. It needs a Rust toolchain, and,
# for front/'s compiler, Meadow (https://github.com/meadow-lang/meadow).
#
# Three binaries are built, and installed in ~/.idyll/bin:
#
#   idyll        the driver (idyll-driver/): the REPL, and the build system
#   idyllic      the compiler (compiler/), which checks a single file
#   idyll-front  front/'s compiler, written in Meadow; it is also the language
#                server the VS Code extension runs (`idyll-front lsp`)
#
# front/'s binary is called `Idyll` where it is built, and gets a name of its
# own here: on Windows and macOS file names ignore case, so `Idyll` and the
# driver's `idyll` cannot share a directory. Without `meadow` on PATH it is
# skipped, and the other two are installed all the same.
#
# The source is the checkout this script is in, when it is in one. Otherwise --
# piped from curl, or asked for a `--version` -- it is cloned into
# ~/.idyll/src, and that is all that is downloaded. `--local <path>` builds a
# checkout somewhere else.
#
# The PATH is edited as meadowup edits it for Meadow: a file, ~/.idyll/env,
# adds the bin directory, and each shell profile that exists gets one line that
# sources it, under a marker comment, so a rerun finds the line and leaves it.
# On Windows the directory is also added to the user PATH in the registry, which
# is what a terminal other than Git Bash reads. `--uninstall` undoes both.
#
# Working on Idyll itself? `--with-extension` builds and installs the VS Code
# extension alongside it.
#
# Run with --help for the options.

set -eu

REPO="meadow-lang/idyll"
IDYLL_HOME="${IDYLL_HOME:-$HOME/.idyll}"

VERSION="latest"
LOCAL=""
UNINSTALL=0
EXTENSION=0
MODIFY_PATH=1

# What a profile carries above the line that sources the env file. The line
# itself, which names the env file, is what a rerun and the removal look for.
MARK="# added by the idyll installer"

main() {
    parse_args "$@"

    # Everything below names files under IDYLL_HOME, some of them in files that
    # outlive this run (the profiles, the registry), so the path must be whole.
    if [ "$UNINSTALL" -eq 1 ]; then
        [ -d "$IDYLL_HOME" ] || err "idyll is not installed at $IDYLL_HOME"
    fi
    mkdir -p "$IDYLL_HOME"
    IDYLL_HOME="$(cd "$IDYLL_HOME" && pwd)"
    BIN_DIR="$IDYLL_HOME/bin"
    ENV_FILE="$IDYLL_HOME/env"

    # Uninstalling builds nothing, so it needs nothing but this script.
    if [ "$UNINSTALL" -eq 1 ]; then
        uninstall
        exit 0
    fi

    build_everything
}

# --- argument parsing -------------------------------------------------------

parse_args() {
    while [ $# -gt 0 ]; do
        case "$1" in
            --version)
                [ $# -ge 2 ] || err "--version needs a tag, e.g. --version v0.1.0-alpha"
                VERSION="$2"
                shift 2
                ;;
            --local)
                [ $# -ge 2 ] || err "--local needs the path to an idyll checkout"
                LOCAL="$2"
                shift 2
                ;;
            --with-extension) EXTENSION=1; shift ;;
            --no-modify-path) MODIFY_PATH=0; shift ;;
            --uninstall) UNINSTALL=1; shift ;;
            --help|-h) usage; exit 0 ;;
            *) err "unknown option: $1 (try --help)" ;;
        esac
    done
}

usage() {
    cat <<'EOF'
Builds the Idyll toolchain from source and installs it.

    scripts/install.sh [options]

    --version <TAG>    fetch and build that tag, rather than this checkout
    --local <PATH>     build the checkout at PATH, rather than this one
    --with-extension   also build and install the VS Code extension
    --no-modify-path   do not touch your shell profiles or PATH
    --uninstall        remove Idyll's binaries and undo the PATH entry

Three binaries are built and installed in ~/.idyll/bin:

    idyll          the driver: the REPL, and the build system
    idyllic        the compiler
    idyll-front    front/'s compiler, and the language server

Building needs a Rust toolchain and takes a few minutes; idyll-front also
needs `meadow` on PATH, and is skipped without it. CARGO_TARGET_DIR, if
set, is where cargo builds.

IDYLL_HOME overrides where everything goes (default ~/.idyll).
EOF
}

# --- building ---------------------------------------------------------------

# Build the three binaries, install them, and put them on the PATH.
#
# Everything is built before anything is installed, so a build that fails
# leaves what was installed before as it was.
build_everything() {
    need_cmd cargo
    src="$(toolchain_source)"
    target="$(target_dir "$src")"

    # From the checkout's root, so cargo reads its .cargo/config.toml: all
    # three Rust workspaces build into the one target/ there, and the tests'
    # stack size is set there too. `--manifest-path` alone would not find it,
    # since cargo looks for its configuration from where it is run.
    say "building idyll, the driver (this takes a few minutes)"
    (cd "$src" && cargo build --release --manifest-path idyll-driver/Cargo.toml --bin idyll) \
        || err "could not build idyll"

    say "building idyllic, the compiler"
    (cd "$src" && cargo build --release --manifest-path compiler/Cargo.toml -p idyllic --bin idyllic) \
        || err "could not build idyllic"

    driver="$(built "$target/release/idyll")" \
        || err "idyll built, but is not in $target/release"
    compiler="$(built "$target/release/idyllic")" \
        || err "idyllic built, but is not in $target/release"

    front=""
    if command -v meadow >/dev/null 2>&1; then
        say "building front/'s compiler"
        (cd "$src/front" && meadow build --release) || err "could not build front/"
        front="$(built "$src/front/target/release/native/silo/Idyll")" \
            || err "front/ built, but there is no front/target/release/native/silo/Idyll"
    else
        say ""
        say "\`meadow\` is not on PATH, so front/'s compiler (idyll-front, which is"
        say "also the language server) is not built. Install Meadow and run this"
        say "again for it: https://github.com/meadow-lang/meadow"
        say ""
    fi

    mkdir -p "$BIN_DIR"
    install_bin "$driver" idyll
    install_bin "$compiler" idyllic
    if [ -n "$front" ]; then
        install_bin "$front" idyll-front
    fi

    if [ "$MODIFY_PATH" -eq 1 ]; then
        add_to_path
    fi
    say_how_to_use

    if [ "$EXTENSION" -eq 1 ]; then
        install_extension "$src"
    fi
    return 0
}

# Where cargo puts what it builds from the checkout `$1`: CARGO_TARGET_DIR if
# it is set, which overrides .cargo/config.toml, and the checkout's target/
# otherwise. A relative one is relative to where cargo ran, the checkout.
target_dir() {
    dir="${CARGO_TARGET_DIR:-target}"
    case "$dir" in
        /*|[A-Za-z]:*) echo "$dir" ;;
        *) echo "$1/$dir" ;;
    esac
}

# The file built at `$1`, which on Windows is `$1.exe`. Fails if neither is.
#
# `.exe` is looked for first: Git Bash answers `[ -f foo ]` for a `foo.exe`
# too, and the name has to be the real one for the suffix to be kept.
built() {
    if [ -f "$1.exe" ]; then
        echo "$1.exe"
    elif [ -f "$1" ]; then
        echo "$1"
    else
        return 1
    fi
}

# Install the binary `$1` in the bin directory as `$2`, keeping its `.exe`.
#
# Copied in beside the old one and then renamed over it, so nothing ever runs a
# half-copied binary. On Windows a program that is running -- the editor's
# language server, say -- cannot be replaced, only renamed, so the old one is
# moved aside first and deleted the next time round. The `.exe` stays last in
# each name, since Git Bash's cp adds one to a copy of an `.exe` whose name
# does not end in it.
install_bin() {
    case "$1" in
        *.exe) exe=".exe" ;;
        *) exe="" ;;
    esac
    dest="$BIN_DIR/$2$exe"
    new="$BIN_DIR/$2.new$exe"
    old="$BIN_DIR/$2.old$exe"
    rm -f "$old" 2>/dev/null || true
    cp "$1" "$new" || err "could not copy $1 to $BIN_DIR"
    chmod +x "$new"
    if ! mv -f "$new" "$dest" 2>/dev/null; then
        mv -f "$dest" "$old" && mv -f "$new" "$dest" \
            || err "could not replace $dest (is it running?)"
    fi
    say "installed $dest"
}

say_how_to_use() {
    say ""
    if [ "$MODIFY_PATH" -eq 0 ]; then
        say "Add this to your PATH: $BIN_DIR"
    elif on_windows; then
        say "Open a new terminal, or in this one run:  . \"$ENV_FILE\""
    else
        say "Open a new shell, or run:  . \"$ENV_FILE\""
    fi
    say "Then \`idyll\` starts the REPL; \`idyll --help\` says what else it does."
}

# Build the VS Code extension from `$1` and install it, for someone working on
# Idyll who wants the editor to follow what they just built.
install_extension() {
    src="$1"
    dir=editors/vscode
    [ -x "$src/$dir/build.sh" ] || err "$src has no $dir/build.sh"
    say ""
    say "building $dir"
    (cd "$src" && "./$dir/build.sh") || err "could not build $dir"

    vsix="$(ls -1 "$src/$dir"/*.vsix 2>/dev/null | head -1)"
    [ -n "$vsix" ] || err "$dir built but produced no .vsix"

    if command -v code >/dev/null 2>&1; then
        say "installing $(basename "$vsix")"
        # `--force` because the version usually has not changed between two
        # local builds, and without it VS Code declines to reinstall it.
        code --install-extension "$vsix" --force >/dev/null \
            || err "could not install $(basename "$vsix")"
        say ""
        if [ -n "$front" ]; then
            say "Reload the editor window (Developer: Reload Window) so the language"
            say "server restarts on the idyll-front just built."
        else
            say "Without idyll-front the extension only highlights; build it (with"
            say "\`meadow\` on PATH) for the language server."
        fi
    else
        say ""
        say "\`code\` is not on PATH, so it was built but not installed:"
        say "  code --install-extension $vsix --force"
    fi
}

# Where to build from: a checkout named with --local, the checkout this script
# is in, or one fetched into $IDYLL_HOME/src. Fetching into the same place each
# time means a second run is an incremental build rather than a clean one.
toolchain_source() {
    if [ -n "$LOCAL" ]; then
        [ -f "$LOCAL/idyll-driver/Cargo.toml" ] && [ -f "$LOCAL/compiler/Cargo.toml" ] \
            || err "$LOCAL does not look like an idyll checkout (no idyll-driver/ and compiler/)"
        (cd "$LOCAL" && pwd)
        return
    fi

    # Run from inside a checkout, build that checkout: "from source" should mean
    # the source in front of you, not a fresh clone of master.
    #
    # `[ -f "$0" ]` is what tells a checkout apart from `curl … | sh`, where the
    # script is not a file on disk at all and the directory above it means
    # nothing.
    if [ "$VERSION" = "latest" ] && [ -f "$0" ]; then
        here="$(cd "$(dirname "$0")/.." 2>/dev/null && pwd)" || here=""
        if [ -n "$here" ] && [ -f "$here/idyll-driver/Cargo.toml" ] \
            && [ -f "$here/compiler/Cargo.toml" ]; then
            say "building this checkout: $here" >&2
            echo "$here"
            return
        fi
    fi

    need_cmd git
    src="$IDYLL_HOME/src"
    if [ "$VERSION" = "latest" ]; then
        ref="HEAD"
        say "fetching the latest source into $src" >&2
    else
        ref="$VERSION"
        say "fetching $VERSION into $src" >&2
    fi
    if [ -d "$src/.git" ]; then
        # The ref asked for, not whatever the clone last had: a second run with
        # a different `--version` must build that version.
        git -C "$src" fetch --depth 1 origin "$ref" >/dev/null 2>&1 \
            || err "git fetch failed (no such tag: $VERSION?)"
        git -C "$src" reset --hard FETCH_HEAD >/dev/null 2>&1 || err "git reset failed"
    else
        rm -rf "$src"
        if [ "$VERSION" = "latest" ]; then
            git clone --depth 1 "https://github.com/$REPO.git" "$src" >/dev/null 2>&1 \
                || err "git clone failed"
        else
            git clone --depth 1 --branch "$VERSION" "https://github.com/$REPO.git" "$src" \
                >/dev/null 2>&1 || err "git clone failed (no such tag: $VERSION?)"
        fi
    fi
    echo "$src"
}

# --- uninstalling -----------------------------------------------------------

# Remove the three binaries and undo the PATH edits. IDYLL_HOME itself stays:
# besides bin/ it holds what projects share (git/, the compiled Std in
# cache/), which `idyll` put there and may still want.
uninstall() {
    removed=0
    for name in idyll idyllic idyll-front; do
        # `.exe` first, for the reason `built` gives.
        for exe in .exe ""; do
            f="$BIN_DIR/$name$exe"
            if [ -f "$f" ]; then
                rm -f "$f" || err "could not remove $f (is it running?)"
                say "removed $f"
                removed=1
            fi
            rm -f "$BIN_DIR/$name.old$exe" "$BIN_DIR/$name.new$exe" 2>/dev/null || true
        done
    done
    [ "$removed" -eq 1 ] || say "no Idyll binaries in $BIN_DIR"
    rmdir "$BIN_DIR" 2>/dev/null || true

    if [ "$MODIFY_PATH" -eq 1 ]; then
        remove_from_path
    fi
    say ""
    say "Left in place, since it holds what projects share: $IDYLL_HOME"
    say "Delete it to remove everything."
}

# --- the PATH ---------------------------------------------------------------

# The line a profile carries. It names the env file, which is what a rerun and
# the removal look for.
source_line() {
    echo ". \"$ENV_FILE\""
}

# Add the bin directory to the PATH: the env file, the profiles that source it,
# and on Windows the user PATH in the registry.
#
# The profiles are those a login or interactive shell reads. All of them that
# exist are edited: which one is read depends on the shell and on how it was
# started, and adding a line to each is cheaper than being wrong.
add_to_path() {
    write_env
    line="$(source_line)"
    any=0
    for profile in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.bash_profile"; do
        [ -f "$profile" ] || continue
        any=1
        if grep -qxF "$line" "$profile"; then
            continue
        fi
        printf '\n%s\n%s\n' "$MARK" "$line" >> "$profile" \
            || err "could not write $profile"
        say "added $BIN_DIR to PATH in $profile"
    done
    # No profile at all: make the one every shell reads, so a new terminal
    # still finds idyll.
    if [ "$any" -eq 0 ]; then
        printf '%s\n%s\n' "$MARK" "$line" > "$HOME/.profile" \
            || err "could not write $HOME/.profile"
        say "added $BIN_DIR to PATH in $HOME/.profile"
    fi

    if on_windows; then
        windows_user_path add
    fi
}

# Write the file the profiles source.
#
# It guards against adding the directory twice, so re-sourcing it -- which a
# shell does on every new terminal -- does not grow PATH.
write_env() {
    cat > "$ENV_FILE" <<EOF || err "could not write $ENV_FILE"
#!/bin/sh
# Adds idyll to PATH. Sourced from your shell profile.
case ":\${PATH}:" in
    *:"$BIN_DIR":*) ;;
    *) export PATH="$BIN_DIR:\$PATH" ;;
esac
EOF
}

# Take out what `add_to_path` put in: the line in each profile, with the marker
# above it and the blank line above that, the env file, and the registry entry.
remove_from_path() {
    line="$(source_line)"
    for profile in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.bash_profile"; do
        [ -f "$profile" ] || continue
        grep -qxF "$line" "$profile" || continue
        tmp="$profile.idyll-uninstall"
        awk -v mark="$MARK" -v line="$line" '
            { l[NR] = $0 }
            END {
                for (i = 1; i <= NR; i++) if (l[i] == line) {
                    drop[i] = 1
                    if (i > 1 && l[i-1] == mark) {
                        drop[i-1] = 1
                        if (i > 2 && l[i-2] == "") drop[i-2] = 1
                    }
                }
                for (i = 1; i <= NR; i++) if (!drop[i]) print l[i]
            }' "$profile" > "$tmp" || err "could not read $profile"
        # Written back through the file rather than moved over it, which keeps
        # its permissions, and a profile that is a symlink stays one.
        cat "$tmp" > "$profile" || err "could not write $profile"
        rm -f "$tmp"
        say "removed $BIN_DIR from PATH in $profile"
    done
    rm -f "$ENV_FILE"

    if on_windows; then
        windows_user_path remove
    fi
}

on_windows() {
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) return 0 ;;
        *) return 1 ;;
    esac
}

# Add the bin directory to the user PATH in the registry, or remove it from
# there: `$1` is `add` or `remove`. Git Bash reads the profiles, but cmd,
# PowerShell, and a VS Code started from the Start menu read only this.
#
# The value is read raw, as meadowup reads it: PATH is often a REG_EXPAND_SZ
# holding entries like `%JAVA_HOME%\bin`, and reading it expanded would write
# the expansion back for good. An entry is the same directory whatever its case
# and however many slashes end it. Nothing is written when the directory is
# already there (to add) or is not (to remove).
#
# [Environment]::SetEnvironmentVariable writes it, and tells running programs
# the environment changed, so a new terminal sees it without a logout. But the
# PowerShell that comes with Windows writes it as REG_SZ, where `%VARS%` are no
# longer expanded; so a value that has some is written through the registry
# instead, and the change announced by clearing a variable that does not exist,
# which SetEnvironmentVariable announces all the same.
#
# The directory and the action are passed in the environment, which needs no
# quoting, and MSYS2_ARG_CONV_EXCL keeps Git Bash from taking the script for
# paths to convert.
windows_user_path() {
    IDYLL_PATH_ACTION="$1" IDYLL_PATH_DIR="$(cygpath -w "$BIN_DIR")" MSYS2_ARG_CONV_EXCL='*' \
        powershell.exe -NoProfile -NonInteractive -Command "$WINDOWS_USER_PATH" \
        || err "could not $1 $BIN_DIR in the user PATH"
}

WINDOWS_USER_PATH='
$ErrorActionPreference = "Stop"
$dir = $env:IDYLL_PATH_DIR
$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)
$old = [string]$key.GetValue("Path", "", "DoNotExpandEnvironmentNames")
$slashes = [char[]]@([char]92, [char]47)
$entries = @($old -split ";" | Where-Object { $_ -ne "" })
$others = @($entries | Where-Object { $_.TrimEnd($slashes) -ine $dir.TrimEnd($slashes) })
$there = $others.Count -lt $entries.Count
if ($env:IDYLL_PATH_ACTION -eq "add") {
    if ($there) { exit 0 }
    $new = ($entries + $dir) -join ";"
    $done = "added $dir to the user PATH"
} else {
    if (-not $there) { exit 0 }
    $new = $others -join ";"
    $done = "removed $dir from the user PATH"
}
if ($new -match "%") {
    $key.SetValue("Path", $new, [Microsoft.Win32.RegistryValueKind]::ExpandString)
    [Environment]::SetEnvironmentVariable("IDYLL_PATH_CHANGED", $null, "User")
} else {
    [Environment]::SetEnvironmentVariable("Path", $new, "User")
}
$done
'

# --- helpers ----------------------------------------------------------------

need_cmd() {
    command -v "$1" >/dev/null 2>&1 || err "need $1"
}

# printf rather than echo, which in some shells (dash, the /bin/sh of Debian
# and Ubuntu) reads backslashes in what it prints -- and a Windows path is full
# of them.
say() {
    printf '%s\n' "$1"
}

err() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

main "$@"
