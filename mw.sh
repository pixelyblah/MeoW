#!/bin/bash

FILE="$1"
[ -z "$FILE" ] && { echo "Usage: mw <filename>"; exit 1; }

# Locate script directory and source external syntax engine
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -f "$SCRIPT_DIR/syntax.sh" ]; then
    source "$SCRIPT_DIR/syntax.sh"
else
    detect_filetype() { filetype="plain"; }
    highlight_line() { buf+="$1"; }
fi

detect_filetype "$FILE"

lines=()
if [ -f "$FILE" ]; then
    while IFS= read -r line || [ -n "$line" ]; do
        lines+=("$line")
    done < "$FILE"
fi
[ ${#lines[@]} -eq 0 ] && lines=("")

cursor_r=0
cursor_c=0
scroll_top=0
mode="NORMAL"
status_msg="\"$FILE\" [${#lines[@]} lines]"
last_key=""

orig_stty=$(stty -g)
cleanup() {
    stty "$orig_stty"
    clear
    echo -e "\e[?25h"
}
trap cleanup EXIT
stty raw -echo

redraw() {
    term_rows=$(tput lines)
    view_height=$((term_rows - 2))

    if [ $cursor_r -lt $scroll_top ]; then
        scroll_top=$cursor_r
    elif [ $cursor_r -ge $((scroll_top + view_height)) ]; then
        scroll_top=$((cursor_r - view_height + 1))
    fi

    # DOUBLE BUFFERING: Accumulate whole frame in $buf
    local buf="\033[H"

    for ((i=0; i<view_height; i++)); do
        line_idx=$((scroll_top + i))
        buf+="\033[$((i + 1));1H\033[K"
        if [ $line_idx -lt ${#lines[@]} ]; then
            highlight_line "${lines[$line_idx]}"
        else
            buf+="~"
        fi
    done

    # Status Bar
    buf+="\033[$((term_rows - 1));1H--------------------------------------------------------\033[K"
    buf+="\033[${term_rows};1H\033[7m -- ${mode} -- | ${FILE} [${filetype}] | ${status_msg}\033[K\033[0m"

    # Set Cursor Position
    screen_r=$((cursor_r - scroll_top + 1))
    buf+="\033[${screen_r};$((cursor_c + 1))H"

    # Output frame in a single stdout write (Zero Flicker)
    printf "%b" "$buf"
}

save_file() {
    > "$FILE"
    for line in "${lines[@]}"; do
        echo "$line" >> "$FILE"
    done
    status_msg="\"$FILE\" written"
}

delete_current_line() {
    if [ ${#lines[@]} -gt 1 ]; then
        lines=("${lines[@]:0:$cursor_r}" "${lines[@]:$((cursor_r+1))}")
        [ $cursor_r -ge ${#lines[@]} ] && cursor_r=$((${#lines[@]} - 1))
        cursor_c=0
        status_msg="Line deleted"
    else
        lines=("")
        cursor_c=0
        status_msg="Buffer cleared"
    fi
}

execute_command_mode() {
    term_rows=$(tput lines)
    printf "\033[%d;1H\033[K:" "$term_rows"
    
    cmd=""
    while true; do
        IFS= read -rsn1 c
        if [ "$c" == $'\x0a' ] || [ "$c" == "" ]; then
            break
        elif [ "$c" == $'\x7f' ] || [ "$c" == $'\x08' ]; then
            if [ ${#cmd} -gt 0 ]; then
                cmd="${cmd:0:-1}"
                echo -e -n "\b \b"
            fi
        elif [ "$c" == $'\x1b' ]; then
            cmd=""
            break
        else
            cmd="${cmd}${c}"
            echo -n "$c"
        fi
    done

    case "$cmd" in
        w) save_file ;;
        q) exit 0 ;;
        "q!") exit 0 ;;
        wq|x) save_file; exit 0 ;;
        "") status_msg="" ;;
        *) status_msg="Unknown command: :$cmd" ;;
    esac
    mode="NORMAL"
}

# Main Loop
while true; do
    redraw
    
    IFS= read -rsn1 key

    if [ "$key" == $'\x1b' ]; then
        read -rsn2 -t 0.01 seq
        if [ "$seq" == "[A" ]; then
            [ $cursor_r -gt 0 ] && ((cursor_r--))
        elif [ "$seq" == "[B" ]; then
            [ $cursor_r -lt $((${#lines[@]} - 1)) ] && ((cursor_r++))
        elif [ "$seq" == "[C" ]; then
            line_len=${#lines[$cursor_r]}
            [ $cursor_c -lt $line_len ] && ((cursor_c++))
        elif [ "$seq" == "[D" ]; then
            [ $cursor_c -gt 0 ] && ((cursor_c--))
        else
            mode="NORMAL"
        fi
        
        [ $cursor_c -gt ${#lines[$cursor_r]} ] && cursor_c=${#lines[$cursor_r]}
        last_key=""
        continue
    fi

    if [ "$mode" == "NORMAL" ]; then
        case "$key" in
            i) mode="INSERT" ;;
            a) mode="INSERT"; ((cursor_c++)) ;;
            I) mode="INSERT"; cursor_c=0 ;;
            A) mode="INSERT"; cursor_c=${#lines[$cursor_r]} ;;
            o) 
                lines=("${lines[@]:0:$((cursor_r+1))}" "" "${lines[@]:$((cursor_r+1))}")
                ((cursor_r++))
                cursor_c=0
                mode="INSERT"
                ;;
            O)
                lines=("${lines[@]:0:$cursor_r}" "" "${lines[@]:$cursor_r}")
                cursor_c=0
                mode="INSERT"
                ;;
            h) [ $cursor_c -gt 0 ] && ((cursor_c--)) ;;
            j) [ $cursor_r -lt $((${#lines[@]} - 1)) ] && ((cursor_r++)) ;;
            k) [ $cursor_r -gt 0 ] && ((cursor_r--)) ;;
            l) [ $cursor_c -lt ${#lines[$cursor_r]} ] && ((cursor_c++)) ;;
            0) cursor_c=0 ;;
            '$') cursor_c=${#lines[$cursor_r]} ;;
            G) cursor_r=$((${#lines[@]} - 1)) ;;
            g)
                if [ "$last_key" == "g" ]; then
                    cursor_r=0
                    last_key=""
                else
                    last_key="g"
                    continue
                fi
                ;;
            x)
                curr_line="${lines[$cursor_r]}"
                if [ ${#curr_line} -gt 0 ]; then
                    lines[$cursor_r]="${curr_line:0:$cursor_c}${curr_line:$((cursor_c+1))}"
                fi
                ;;
            d) 
                if [ "$last_key" == "d" ]; then
                    delete_current_line
                    last_key=""
                else
                    last_key="d"
                    continue
                fi
                ;;
            :) execute_command_mode ;;
        esac
        
        [ $cursor_c -gt ${#lines[$cursor_r]} ] && cursor_c=${#lines[$cursor_r]}
        [ "$key" != "d" ] && [ "$key" != "g" ] && last_key=""

    elif [ "$mode" == "INSERT" ]; then
        curr_line="${lines[$cursor_r]}"
        
        if [ "$key" == $'\x7f' ] || [ "$key" == $'\x08' ]; then
            if [ $cursor_c -gt 0 ]; then
                lines[$cursor_r]="${curr_line:0:$((cursor_c-1))}${curr_line:$cursor_c}"
                ((cursor_c--))
            elif [ $cursor_r -gt 0 ]; then
                prev_line="${lines[$((cursor_r-1))]}"
                cursor_c=${#prev_line}
                lines[$((cursor_r-1))]="${prev_line}${curr_line}"
                lines=("${lines[@]:0:$cursor_r}" "${lines[@]:$((cursor_r+1))}")
                ((cursor_r--))
            fi
        elif [ "$key" == $'\x0a' ] || [ "$key" == "" ]; then
            left="${curr_line:0:$cursor_c}"
            right="${curr_line:$cursor_c}"
            lines[$cursor_r]="$left"
            lines=("${lines[@]:0:$((cursor_r+1))}" "$right" "${lines[@]:$((cursor_r+1))}")
            ((cursor_r++))
            cursor_c=0
        else
            left="${curr_line:0:$cursor_c}"
            right="${curr_line:$cursor_c}"
            lines[$cursor_r]="${left}${key}${right}"
            ((cursor_c++))
        fi
    fi
done
