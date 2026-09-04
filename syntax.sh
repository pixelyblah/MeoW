x.sh - Syntax Highlighting & Language Logic for mw

detect_filetype() {
    case "$1" in
        *.sh|*.bash) filetype="bash" ;;
        Makefile|makefile|*.mk) filetype="make" ;;
        *) filetype="plain" ;;
    esac
}

highlight_line() {
    local line="$1"
    [ "$filetype" == "plain" ] && { buf+="$line"; return; }

    # Comments (Gray)
    if [[ "$line" =~ ^([[:space:]]*)(#.*)$ ]]; then
        buf+="${BASH_REMATCH[1]}\033[38;2;110;115;141m${BASH_REMATCH[2]}\033[0m"
        return
    fi

    if [ "$filetype" == "make" ]; then
        # Makefile Targets (Yellow)
        if [[ "$line" =~ ^([a-zA-Z0-9_\.-]+):(.*)$ ]]; then
            buf+="\033[33;1m${BASH_REMATCH[1]}\033[0m:${BASH_REMATCH[2]}"
            return
        fi
    elif [ "$filetype" == "bash" ]; then
        # Bash Keywords (Magenta)
        if [[ "$line" =~ ^([[:space:]]*)(if|then|else|elif|fi|for|while|do|done|case|esac|function|return|exit)([[:space:]]+.*|;.*|)$ ]]; then
            buf+="${BASH_REMATCH[1]}\033[35;1m${BASH_REMATCH[2]}\033[0m${BASH_REMATCH[3]}"
            return
        fi
    fi

    buf+="$line"
}
