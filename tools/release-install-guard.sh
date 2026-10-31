# Explorer and the full bundle use different receipts. Transfer ownership only
# after every binary and the cargo-dist receipt have been installed successfully.
ESKA_FULL_STAGE=""
ESKA_FULL_READY=0
ESKA_FULL_COMMITTED=0

eska_full_error() {
    case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
        ru*) err "$1" ;;
        *) err "$2" ;;
    esac
}

eska_restore_full_file() {
    local _eska_backup="$1"
    local _eska_destination="$2"
    if [ -L "$_eska_destination" ] || { [ -e "$_eska_destination" ] && [ ! -f "$_eska_destination" ]; }; then
        return 1
    fi
    if [ -f "$_eska_backup" ]; then
        cmp -s "$_eska_backup" "$_eska_destination" || cp -p "$_eska_backup" "$_eska_destination"
    elif [ -e "$_eska_destination" ]; then
        rm -f "$_eska_destination"
    fi
}

eska_finish_full_install() {
    local _eska_status="$1"
    trap - 0 HUP INT TERM
    if [ -n "$ESKA_FULL_STAGE" ]; then
        local _eska_ok=1
        local _eska_name
        if [ "$ESKA_FULL_READY" = 1 ] && [ "$ESKA_FULL_COMMITTED" != 1 ]; then
            for _eska_name in $ESKA_FULL_BINS; do
                eska_restore_full_file "$ESKA_FULL_STAGE/bin/$_eska_name" "$ESKA_FULL_BIN/$_eska_name" || _eska_ok=0
            done
            for _eska_name in $ESKA_FULL_COMPONENTS; do
                eska_restore_full_file "$ESKA_FULL_STAGE/components/$_eska_name.json" "$ESKA_FULL_ROOT/components/$_eska_name.json" || _eska_ok=0
            done
            eska_restore_full_file "$ESKA_FULL_STAGE/installer-receipt.json" "$ESKA_FULL_RECEIPT" || _eska_ok=0
        fi
        if [ "$_eska_ok" != 1 ]; then
            eska_full_error \
                "component-installation-rollback: Не удалось восстановить прежнюю установку. Резервные копии: $ESKA_FULL_STAGE" \
                "component-installation-rollback: Could not restore the previous installation. Backups: $ESKA_FULL_STAGE"
        fi
        if [ -f "$ESKA_FULL_STAGE/claim" ] && cmp -s "$ESKA_FULL_STAGE/claim" "$ESKA_FULL_ROOT/.eska-components.claim"; then
            rm -f "$ESKA_FULL_ROOT/.eska-components.claim" || _eska_status=1
        fi
        rm -rf "$ESKA_FULL_STAGE" || _eska_status=1
    fi
    exit "$_eska_status"
}

eska_prepare_full_install() {
    local _eska_bin="$1"
    if [ -d "$_eska_bin" ]; then
        _eska_bin="$(CDPATH='' cd "$_eska_bin" && pwd -P)"
    fi
    [ "${_eska_bin##*/}" = bin ] || return 0
    local _eska_root="${_eska_bin%/*}"
    local _eska_name
    local _eska_components=""
    for _eska_name in eska eska-project eska-check eska-build eska-vcs eska-fmt; do
        if [ -e "$_eska_root/components/$_eska_name.json" ] || [ -L "$_eska_root/components/$_eska_name.json" ]; then
            _eska_components="$_eska_components $_eska_name"
        fi
    done
    [ -n "$_eska_components" ] || return 0
    if [ "$INSTALL_UPDATER" != 1 ]; then
        eska_full_error \
            "component-installation-receipt-required: Для перехода с Explorer на полный комплект запустите установщик без ESKA_DISABLE_UPDATE и ESKA_UNMANAGED_INSTALL." \
            "component-installation-receipt-required: To replace Explorer components with the full bundle, run without ESKA_DISABLE_UPDATE and ESKA_UNMANAGED_INSTALL."
    fi
    if [ -L "$_eska_root/components" ] || [ ! -d "$_eska_root/components" ]; then
        eska_full_error "component-installation-path: Недопустимый каталог $_eska_root/components" \
            "component-installation-path: Unsafe directory $_eska_root/components"
    fi
    ESKA_FULL_ROOT="$_eska_root"
    ESKA_FULL_BIN="$_eska_bin"
    ESKA_FULL_BINS="$2"
    ESKA_FULL_COMPONENTS="$_eska_components"
    ESKA_FULL_RECEIPT="$RECEIPT_HOME/$APP_NAME-receipt.json"
    trap 'eska_finish_full_install "$?"' 0
    trap 'exit 129' HUP
    trap 'exit 130' INT
    trap 'exit 143' TERM
    ESKA_FULL_STAGE="$(mktemp -d "$_eska_root/.eska-full-install.XXXXXXXX")" || exit 1
    local _eska_nonce
    _eska_nonce="$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')"
    [ "${#_eska_nonce}" = 32 ] || exit 1
    printf '{"schema_version":1,"pid":%s,"nonce":"%s"}\n' "$$" "$_eska_nonce" > "$ESKA_FULL_STAGE/claim" || exit 1
    if ! (set -C; cat "$ESKA_FULL_STAGE/claim" > "$_eska_root/.eska-components.claim") 2>/dev/null; then
        eska_full_error "component-installation-busy: Дождитесь завершения установки Explorer: $_eska_root" \
            "component-installation-busy: Wait for the Explorer installation to finish: $_eska_root"
    fi
    # A stale claim or interrupted component transaction needs its original recovery.
    for _eska_name in "$_eska_root/.eska-components.recovering" "$_eska_root"/.eska-component-*/journal.json; do
        if [ -e "$_eska_name" ] || [ -L "$_eska_name" ]; then
            eska_full_error "component-installation-busy: Сначала восстановите прерванную установку: $_eska_name" \
                "component-installation-busy: Recover the interrupted installation first: $_eska_name"
        fi
    done
    # Another component may have finished installing before this claim was acquired.
    ESKA_FULL_COMPONENTS=""
    for _eska_name in eska eska-project eska-check eska-build eska-vcs eska-fmt; do
        if [ -e "$_eska_root/components/$_eska_name.json" ] || [ -L "$_eska_root/components/$_eska_name.json" ]; then
            ESKA_FULL_COMPONENTS="$ESKA_FULL_COMPONENTS $_eska_name"
        fi
    done
    ensure mkdir "$ESKA_FULL_STAGE/bin" "$ESKA_FULL_STAGE/components"
    for _eska_name in $ESKA_FULL_BINS; do
        local _eska_path="$_eska_bin/$_eska_name"
        if [ -L "$_eska_path" ] || { [ -e "$_eska_path" ] && [ ! -f "$_eska_path" ]; }; then
            eska_full_error "component-installation-path: Недопустимый файл $_eska_path" \
                "component-installation-path: Unsafe file $_eska_path"
        fi
        if [ -f "$_eska_path" ]; then
            ensure cp -p "$_eska_path" "$ESKA_FULL_STAGE/bin/$_eska_name"
        fi
    done
    for _eska_name in $ESKA_FULL_COMPONENTS; do
        local _eska_path="$_eska_root/components/$_eska_name.json"
        if [ ! -f "$_eska_path" ] || [ -L "$_eska_path" ]; then
            eska_full_error "component-installation-path: Недопустимый файл $_eska_path" \
                "component-installation-path: Unsafe file $_eska_path"
        fi
        ensure cp -p "$_eska_path" "$ESKA_FULL_STAGE/components/$_eska_name.json"
    done
    if [ -L "$ESKA_FULL_RECEIPT" ] || { [ -e "$ESKA_FULL_RECEIPT" ] && [ ! -f "$ESKA_FULL_RECEIPT" ]; }; then
        eska_full_error "component-installation-path: Недопустимый файл $ESKA_FULL_RECEIPT" \
            "component-installation-path: Unsafe file $ESKA_FULL_RECEIPT"
    fi
    if [ -f "$ESKA_FULL_RECEIPT" ]; then
        ensure cp -p "$ESKA_FULL_RECEIPT" "$ESKA_FULL_STAGE/installer-receipt.json"
    fi
    ESKA_FULL_READY=1
    case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
        ru*) say "Обновление полного комплекта ESKA до $APP_VERSION, включая компоненты Explorer." ;;
        *) say "Updating the complete ESKA bundle to $APP_VERSION, including Explorer components." ;;
    esac
}

eska_commit_full_install() {
    [ -n "$ESKA_FULL_STAGE" ] || return 0
    local _eska_name
    for _eska_name in $ESKA_FULL_COMPONENTS; do
        ensure rm "$ESKA_FULL_ROOT/components/$_eska_name.json"
    done
    ESKA_FULL_COMMITTED=1
}
