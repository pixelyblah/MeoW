#!/bin/bash

# ghostmeow - Pure Bash Vim-like Editor
FILE="$1"

if [ -z "$FILE" ]; then
    echo "Usage: mw <filename>"
    exit 1
fi

# Load file into array buffer
lines=()
if [ -f "$FILE" ]; then
    while IFS= read -r line || [ -n "$line" ]; do
        lines+=("$line")
    done < "$FILE"
fi

# Initialize if file is empty
[ ${#lines[@]} -eq 0 ] && lines=("")

# Cursor & State variables
cursor_r=0
cursor_c=0
mode="NORMAL"
status_msg="\"$FILE\" [${#lines[@]} lines]"
last_key=""

# Terminal setup & cleanup
orig_stty=$(stty -g)
cleanup() {
    stty "$orig_stty"
    clear
    echo -e "\e[?25h" # Restore cursor visibility
}
trap cleanup EXIT

stty raw -echo

redraw() {
    clear
    # Print buffer lines
    for i in "${!lines[@]}"; do
        echo -e -n "\r${lines[$i]}\r\n"
    done

    # Status Bar
    echo -e -n "\r----------------------------------------\r\n"
    if [ "$mode" == "NORMAL" ]; then
        echo -e -n "\r\e[7m -- NORMAL -- | $FILE | $status_msg \e[0m\r\n"
    elif [ "$mode" == "COMMAND" ]; then
        echo -e -n "\r\e[7m -- COMMAND -- | $FILE | Type w, q, or wq \e[0m\r\n"
    else
        echo -e -n "\r\e[7m -- INSERT -- | $FILE | $status_msg \e[0m\r\n"
    fi

    # Position Cursor
    echo -e -n "\e[$((cursor_r + 1));$((cursor_c + 1))H"
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
    redraw
    # Position cursor at bottom for command entry
    echo -e -n "\e[$(( ${#lines[@]} + 3 ));1H\r\e[K:"
    
    cmd=""
    while true; do
        IFS= read -rsn1 c
        # Enter key finishes command
        if [ "$c" == $'\x0a' ] || [ "$c" == "" ]; then
            break
        # Backspace inside command prompt
        elif [ "$c" == $'\x7f' ] || [ "$c" == $'\x08' ]; then
            if [ ${#cmd} -gt 0 ]; then
                cmd="${cmd:0:-1}"
                echo -e -n "\b \b"
            fi
        # Cancel command on ESC
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

    # Handle Escape sequences (Arrows & ESC key)
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
        
        # Adjust column bounds if switching lines
        [ $cursor_c -gt ${#lines[$cursor_r]} ] && cursor_c=${#lines[$cursor_r]}
        last_key=""
        continue
    fi

    # MODE: NORMAL
    if [ "$mode" == "NORMAL" ]; then
        case "$key" in
            i) mode="INSERT" ;;
            a) mode="INSERT"; ((cursor_c++)) ;;
            h) [ $cursor_c -gt 0 ] && ((cursor_c--)) ;;
            j) [ $cursor_r -lt $((${#lines[@]} - 1)) ] && ((cursor_r++)) ;;
            k) [ $cursor_r -gt 0 ] && ((cursor_r--)) ;;
            l) [ $cursor_c -lt ${#lines[$cursor_r]} ] && ((cursor_c++)) ;;
            :) execute_command_mode ;;
            d) 
                if [ "$last_key" == "d" ]; then
                    delete_current_line
                    last_key=""
                else
                    last_key="d"
                    continue
                fi
                ;;
        esac
        
        # Keep cursor within valid boundary
        [ $cursor_c -gt ${#lines[$cursor_r]} ] && cursor_c=${#lines[$cursor_r]}
        [ "$key" != "d" ] && last_key=""

    # MODE: INSERT
    elif [ "$mode" == "INSERT" ]; then
        curr_line="${lines[$cursor_r]}"
        
        # Backspace handling
        if [ "$key" == $'\x7f' ] || [ "$key" == $'\x08' ]; then
            if [ $cursor_c -gt 0 ]; then
                # Delete character behind cursor
                lines[$cursor_r]="${curr_line:0:$((cursor_c-1))}${curr_line:$cursor_c}"
                ((cursor_c--))
            elif [ $cursor_r -gt 0 ]; then
                # MERGE LINES (Remove line space/break above)
                prev_line="${lines[$((cursor_r-1))]}"
                cursor_c=${#prev_line}
                lines[$((cursor_r-1))]="${prev_line}${curr_line}"
                lines=("${lines[@]:0:$cursor_r}" "${lines[@]:$((cursor_r+1))}")
                ((cursor_r--))
            fi

        # Enter key (Insert line break)
        elif [ "$key" == $'\x0a' ] || [ "$key" == "" ]; then
            left="${curr_line:0:$cursor_c}"
            right="${curr_line:$cursor_c}"
            lines[$cursor_r]="$left"
            lines=("${lines[@]:0:$((cursor_r+1))}" "$right" "${lines[@]:$((cursor_r+1))}")
            ((cursor_r++))
            cursor_c=0

        # Typing text
        else
            left="${curr_line:0:$cursor_c}"
            right="${curr_line:$cursor_c}"
            lines[$cursor_r]="${left}${key}${right}"
            ((cursor_c++))
        fi
    fi
done
