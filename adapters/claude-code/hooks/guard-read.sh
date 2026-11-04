#!/bin/sh
# The read guard. Exit 0 allows, exit 2 blocks (before a tool) or tells the agent (after it).
# Never edits a file of the repository.
#
# sh guard-read.sh PreToolUse on Read|Grep|Glob|Bash|Edit|Write: blocks a whole read of
# a never-whole path, a large text file, and a read of a removed
# document from git history; logs every other way around trailmark.
# sh guard-read.sh post PostToolUse on the same tools: after a logged read of the store,
# says which trailmark command answers it and how to record the miss.
#
# The log is one line per read, tab-separated, in the git common directory every worktree
# shares (never committed): <git common dir>/trail/reads.tsv
# time worktree session tool kind verdict target
# kind: whole | partial | grep | shell | edit | history; verdict: blocked | logged | brief.
# A command that carries TRAIL_ALLOW=brief (its brief asked for that read) is let through
# and logged as `brief`. Loops grepped and edited the store, and the guard saw only
# the Read tool.
mode=${1:-pre}
input=$(cat | tr '\n' ' ')
# After a tool ran, the payload also carries its output (`tool_response`): text that was printed,
# not a command that was typed. A command is read from what comes before it.
scan=${input%%\"tool_response\"*}
dir=$(CDPATH= cd -- "$(dirname "$0")" && pwd) || exit 0
list="$dir/never-whole.txt"
root=$(CDPATH= cd -- "$dir/../.." && pwd) || exit 0

field() {
    printf '%s' "$input" | sed -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p"
}
tool=$(field tool_name)
[ -n "$tool" ] || exit 0

# The never-whole list: a line ending in / is the store; a path that no longer exists in the
# checkout is a removed document (its git history is closed too: `git show <rev>:<path>`,
# `git log -p`, `cat-file`, `checkout`/`restore` of it, `blame`; a `git diff` of the working
# tree is not history).
stores=""
gone=""
if [ -f "$list" ]; then
    while IFS= read -r line || [ -n "$line" ]; do
        [ -z "$line" ] && continue
        case "$line" in
            */) stores="$stores $line" ;;
            *\*)
                prefix=${line%%\**}
                set -- "$root/$prefix"*
                [ -e "$1" ] || gone="$gone $prefix"
                ;;
            *) [ -e "$root/$line" ] || gone="$gone $line" ;;
        esac
    done < "$list"
fi
# A missing file under the store is a local cache not built yet (`index/ids.tsv` in a fresh
# clone), not a removed document.
kept=""
for g in $gone; do
    under=0
    for s in $stores; do
        case "$g" in "$s"*) under=1 ;; esac
    done
    [ "$under" -eq 1 ] || kept="$kept $g"
done
gone=$kept

# Most calls name neither the store nor a removed document: leave before any other process
# (the guard runs on every Bash call, twice).
if [ "$tool" != Read ]; then
    hit=0
    for needle in $stores $gone; do
        case "$scan" in *"${needle%/}"*) hit=1 ;; esac
    done
    [ "$hit" -eq 1 ] || exit 0
fi

log() { # kind verdict target
    [ "$mode" = pre ] || return 0
    common=$(git -C "$root" rev-parse --git-common-dir 2>/dev/null) || return 0
    case "$common" in /*) ;; *) common="$root/$common" ;; esac
    mkdir -p "$common/trail" 2>/dev/null || return 0
    cwd=$(field cwd)
    session=$(field agent_id)
    [ -n "$session" ] || session=$(field session_id)
    target=$(printf '%s' "$3" | tr '\t' ' ' | cut -c1-240)
    f="$common/trail/reads.tsv"
    # Capped: past 1 MB (about 5,000 lines, months of reads) the log moves to reads.tsv.1,
    # replacing the one before, so the two never pass 2 MB.
    if [ -f "$f" ] && [ "$(wc -c < "$f" 2>/dev/null | tr -d ' ')" -gt 1000000 ] 2>/dev/null; then
        mv -f "$f" "$f.1" 2>/dev/null
    fi
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "${cwd##*/}" \
        "$session" "$tool" "$1" "$2" "$target" >> "$f" 2>/dev/null
    return 0
}

advise() { # what was logged, said after the tool ran
    [ "$mode" = post ] || return 0
    printf '%s\n' "trailmark: $1 was logged as a read of the store outside trailmark. For a rule or a record: \`trailmark find <words>\`, \`trailmark show <id> [--part <name>]\`; a record changes through \`trailmark set|link|tag|close|retire\`. If trailmark could not answer, record the miss: \`trailmark link <your loop> <id> --rel missed\` and \`trailmark tag <id> --add <your task's word>\`." >&2
    exit 2
}

# The evaluation's cached stores (`TRAIL_EVAL_CACHE`, default ~/.cache/trail/eval)
# are copies rebuilt from git that the evaluation's own controls read: not the store.
eval_cache() { # path [dir it is relative to] → 0 when it lies in the evaluation's cache
    # A `..` can leave the cache (`~/.cache/trail/eval/../../x/docs/records/…`): not the cache.
    case "$1/" in */../*) return 1 ;; esac
    ec=${TRAIL_EVAL_CACHE:-$HOME/.cache/trail/eval}
    case "$1" in
        "$ec"/*|*/.cache/trail/eval/*|"~/.cache/trail/eval/"*) return 0 ;;
        stores/*/*|stores-shuffled/*/*|*/stores/*/docs/*|*/stores-shuffled/*/docs/*)
            case "$1" in /*) ;; *) return 0 ;; esac ;;
    esac
    case "$1" in /*|"~"*) return 1 ;; esac
    base=${2:-$(field cwd)}
    case "$base/" in */../*) return 1 ;; esac
    case "$base/" in "$ec"/*|*/.cache/trail/eval/*|"~/.cache/trail/eval/"*) return 0 ;; esac
    return 1
}

# The files a reading command reads: its operands, without its options, the values
# its options take, and the pattern, script or filter that comes first (`grep -v docs/records`
# reads its input, not the store: a filter over `git status` or `trailmark find` was logged as a
# read). Input redirected from a file (`< path`) is read too. One path per line.
operands() {
    printf '%s\n' "$1" | awk '
    function opt_arg(cmd, o) {
        if (cmd ~ /^(grep|egrep|fgrep)$/) return o ~ /^-[efABCmdD]$/
        if (cmd == "rg" || cmd == "ag") return o ~ /^-[efABCmgtTjM]$/ || o ~ /^--(glob|type|type-not|max-count|max-depth|regexp|file|context|after-context|before-context|max-columns|threads|sort|sortr)$/
        if (cmd == "sed") return o ~ /^-[ef]$/ || o == "--expression"
        if (cmd == "awk") return o ~ /^-[Fvf]$/
        if (cmd == "jq") return o ~ /^-[fL]$/ || o ~ /^--(indent|from-file)$/
        if (cmd == "head" || cmd == "tail") return o ~ /^-[nc]$/
        if (cmd == "cut") return o ~ /^-[dfcb]$/
        if (cmd == "nl") return o ~ /^-[bdfhilnsvw]$/
        return 0
    }
    {
        s = $0; gsub(/\\"/, "\"", s); n = 0; cur = ""; q = ""; have = 0
        for (i = 1; i <= length(s); i++) {
            c = substr(s, i, 1)
            if (q != "") { if (c == q) q = ""; else cur = cur c; continue }
            if (c == "\047" || c == "\"") { q = c; have = 1; continue }
            if (c == " " || c == "\t") { if (have) { T[++n] = cur; cur = ""; have = 0 }; continue }
            cur = cur c; have = 1
        }
        if (have) T[++n] = cur
        k = 1
        if (T[k] == "xargs") { k++; while (k <= n && T[k] ~ /^-/) { if (T[k] ~ /^-[IJLnPs]$/) k++; k++ } }
        cmd = T[k]; sub(/.*\//, "", cmd)
        firstfree = (cmd ~ /^(grep|egrep|fgrep)$/ || cmd == "rg" || cmd == "ag" || cmd == "sed" || cmd == "awk" || cmd == "jq")
        every = (cmd ~ /^python/)
        given = 0; ends = 0
        for (j = k + 1; j <= n; j++) {
            t = T[j]
            if (t == "<" && j < n) { print T[++j]; continue }
            if (t ~ /^[0-9]*[<>]/) { if (t ~ /^<[^<]/) print substr(t, 2); else if (t ~ /^[0-9]*>+$/) j++; continue }
            if (every) { print t; continue }
            if (!ends && t == "--") { ends = 1; continue }
            if (!ends && t ~ /^-./) {
                if (cmd ~ /grep$|^rg$|^ag$/ && (t ~ /^-[ef]/ || t ~ /^--(regexp|file)/)) given = 1
                if (cmd == "sed" && (t ~ /^-[ef]/ || t ~ /^--expression/)) given = 1
                if (cmd == "awk" && t ~ /^-f/) given = 1
                if (cmd == "jq" && (t ~ /^-f$/ || t == "--from-file")) given = 1
                if (cmd == "jq" && t ~ /^--(arg|argjson|slurpfile|rawfile)$/) { j += 2; continue }
                if (cmd == "sed" && t == "-i" && j < n && T[j + 1] == "") { j++; continue }
                if (opt_arg(cmd, t)) j++
                continue
            }
            if (firstfree && !given) { given = 1; if (cmd == "awk" && t ~ /getline/) print t; continue }
            print t
        }
    }'
}

in_store() { # path or words → 0 when they name a record of the store (not its reservations or index)
    for s in $stores; do
        printf '%s' "$1" | grep -Eo "(^|[^!=[:alnum:]_])$s?[^[:space:]\"'|;&)]*" | sed -E 's/^[^[:alnum:]_.~]//' |
            grep -Ev "/(reservations|index)(/|\.toml|$)|/_migration/|^$" | grep -q . && return 0
    done
    return 1
}

case "$tool" in
Read)
    path=$(field file_path)
    [ -n "$path" ] || exit 0
    eval_cache "$path" && exit 0
    partial=0
    printf '%s' "$input" | grep -Eq '"offset"|"limit"' && partial=1

    # A picture is not text: the harness shows it, it is never paged, and its byte count says
    # nothing about what reading it costs. A contact sheet is a picture, not a document.
    lower=$(printf '%s' "$path" | tr '[:upper:]' '[:lower:]')
    case "$lower" in
        *.png|*.jpg|*.jpeg|*.webp|*.gif) exit 0 ;;
    esac

    blocked=0
    if [ -f "$list" ]; then
        while IFS= read -r line || [ -n "$line" ]; do
            [ -z "$line" ] && continue
            case "$line" in
                *\*)
                    prefix=${line%%\**}
                    # The harness passes absolute paths; the list holds repo-relative ones.
                    case "$path" in
                        "$prefix"*|*"/$prefix"*) blocked=1 ;;
                    esac
                    ;;
                */)
                    case "$path" in
                        *"$line"*) blocked=1 ;;
                    esac
                    ;;
                *)
                    case "$path" in
                        "$line"|*"/$line"|*"$line") blocked=1 ;;
                    esac
                    ;;
            esac
        done < "$list"
    fi
    if [ "$blocked" -eq 1 ] && [ "$partial" -eq 0 ]; then
        [ "$mode" = pre ] || exit 0
        log whole blocked "$path"
        printf '%s\n' "Do not read this file whole. Use \`trailmark context\`, \`trailmark show\`, or \`trailmark find\`." >&2
        exit 2
    fi
    if [ "$partial" -eq 1 ] && in_store "$path"; then
        log partial logged "$path"
        advise "this partial read"
        exit 0
    fi
    [ "$mode" = pre ] || exit 0

    # A large text file that trailmark does not index (a source file, a generated one) is read in
    # parts. Only text is counted: `grep -I` treats a file with a NUL byte as binary.
    if [ "$partial" -eq 0 ] && [ -f "$path" ] && grep -Iq . "$path" 2>/dev/null; then
        chars=$(wc -m < "$path" | tr -d ' ')
        if [ "${chars:-0}" -gt 100000 ]; then
            printf '%s\n' "This file is ${chars} characters (over 100,000). Read it in parts with offset and limit, after a grep for the lines you need." >&2
            exit 2
        fi
    fi
    exit 0
    ;;
Grep)
    # The pattern is not a path: `docs/records` searched for in the code is no read of the store.
    where="$(field path) $(field glob)"
    eval_cache "$(field path)" && exit 0
    if in_store "$where"; then
        log grep logged "$where"
        advise "this search"
    fi
    exit 0
    ;;
Glob) exit 0 ;; # a listing of names reads no record's content
Edit|Write|MultiEdit|NotebookEdit)
    path=$(field file_path)
    eval_cache "$path" && exit 0
    if in_store "$path"; then
        log edit logged "$path"
        advise "this edit"
    fi
    exit 0
    ;;
Bash)
    cmd=$(printf '%s' "$scan" | sed -n 's/.*"command"[[:space:]]*:[[:space:]]*"\(.*\)$/\1/p' |
        sed -E 's/"[[:space:]]*,[[:space:]]*"(description|timeout|run_in_background|dangerouslyDisableSandbox)"[[:space:]]*:.*$//' | cut -c1-4000)
    [ -n "$cmd" ] || exit 0
    brief=0
    printf '%s' "$cmd" | grep -q 'TRAIL_ALLOW=brief' && brief=1
    # Each part of a pipeline or a list on its own line: `git diff <record> | grep x` is a
    # diff, not a read of the store. A heredoc's body is text, not commands: a commit message
    # or an incident (`trailmark add incident --body - <<EOF`) that quotes `git log -p -- <removed
    # document>` on a line of its own was blocked. An unterminated
    # heredoc drops the rest: the guard fails open.
    parts=$(printf '%s\n' "$cmd" | awk '{
        gsub(/\\n/, "\n"); n = split($0, L, "\n")
        for (i = 1; i <= n; i++) {
            line = L[i]
            if (word != "") {
                t = line; gsub(/^([ \t]|\\t)+|([ \t]|\\t)+$/, "", t)
                if (t == word) word = ""
                continue
            }
            print line
            if (match(line, /<<-?[ \t]*[\\"'"'"']*[A-Za-z_][A-Za-z_0-9]*/) &&
                (RSTART == 1 || substr(line, RSTART - 1, 1) != "<")) {
                w = substr(line, RSTART, RLENGTH); sub(/^<<-?[ \t]*[\\"'"'"']*/, "", w); word = w
            }
        }
    }' | tr '|;&' '\n\n\n')
    history=""
    shell=""
    edit=""
    cdir=""
    outer=""
    insub=0
    restore=0
    listing=0
    case "$cmd" in
        *\|*) piped=1 ;;
        *) piped=0 ;;
    esac
    while IFS= read -r part; do
        if [ "$restore" -eq 1 ]; then cdir=$outer; insub=0; restore=0; fi
        case "$part" in *\)*) [ "$insub" -eq 1 ] && restore=1 ;; esac
        p=$(printf '%s' "$part" | sed 's/^[[:space:](]*//; s/^\([A-Z_][A-Z_0-9]*=[^ ]* \)*//')
        # `bash -c 'cat <record>'` and `sh -c` hide the reader in the script argument.
        case "$p" in
            bash\ *|sh\ *)
                inner=$(printf '%s' "$p" | sed -nE 's/^(bash|sh)( +-[a-zA-Z]+)* +-c +//p')
                case "$inner" in
                    \'*\') inner=${inner#\'}; inner=${inner%\'};;
                    \"*\") inner=${inner#\"}; inner=${inner%\"};;
                esac
                [ -n "$inner" ] && p=$inner
                ;;
        esac
        case "$p" in
            git\ *)
                for g in $gone; do
                    case "$p" in
                        *"$g"*)
                            # Global options before the verb (`--no-pager`, `-c k=v`) are
                            # history too; `log`'s patch flag is a word of its own, so
                            # `--pretty` is not `-p`.
                            printf '%s' "$p" | grep -Eq '^git(( -[Cc]| --git-dir| --work-tree) [^ ]+| --(no-pager|git-dir=[^ ]+|work-tree=[^ ]+))* (show|cat-file|checkout|restore|blame|grep|log( .*)? (-p|-u|--patch[^ ]*|-L[^ ]*)( |$))' &&
                                history="$p"
                            ;;
                    esac
                done
                ;;
            cd\ *)
                # `(cd dir && …); next` moves only the subshell: cdir goes back at the `)`.
                case "$part" in *\(*) outer=$cdir; insub=1; case "$part" in *\)*) restore=1 ;; esac ;; esac
                cdir=$(printf '%s' "$p" | sed -E 's/^cd +//; s/[[:space:]]+$//; s/^["'"'"']//; s/["'"'"']$//')
                case "$cdir" in "~"*) cdir="$HOME${cdir#\~}";; esac
                ;;
            trailmark\ *|ls\ *|ls|mkdir\ *|cp\ *|mv\ *|rm\ *|echo\ *|printf\ *|test\ *|\[\ *) ;;
            find\ *)
                # A listing is not itself a read. `find <store> -exec cat` is, and so is
                # `find <store> | xargs cat` (the next stage has no path of its own).
                if printf '%s' "$p" | grep -Eq ' -exec(dir)? +(cat|sed|head|tail|grep|egrep|fgrep|rg|awk|less|more|bat|nl|cut|wc|jq)( |$)' && in_store "$p"; then
                    shell="$p"
                elif in_store "$p"; then
                    listing=1
                fi
                ;;
            xargs\ *)
                if [ "$piped" -eq 1 ] && [ "$listing" -eq 1 ] && printf '%s' "$p" | grep -Eq '^xargs( +-[^ ]+)* +(cat|sed|head|tail|grep|egrep|fgrep|rg|awk|less|more|bat|nl|cut|wc|jq)( |$)'; then
                    shell="$p"
                fi
                listing=0
                ;;
            *)
                listing=0
                in_store "$p" || continue
                if printf '%s' "$p" | grep -Eq "sed -i|>[[:space:]]*[^[:space:]]*records/"; then
                    files=$(operands "$p")
                    while IFS= read -r f; do
                        [ -n "$f" ] || continue
                        eval_cache "$f" "$cdir" && continue
                        in_store "$f" && { edit="$p"; break; }
                    done <<OPS
$files
OPS
                    printf '%s' "$p" | grep -Eq ">[[:space:]]*[^[:space:]]*records/" && ! eval_cache "$(printf '%s' "$p" | sed -nE 's/.*>[[:space:]]*([^[:space:]]*records\/[^[:space:]]*).*/\1/p')" "$cdir" && edit="$p"
                elif printf '%s' "$p" | grep -Eq '^(xargs +([^ ]+ +)*)?(cat|sed|head|tail|grep|egrep|fgrep|rg|ag|awk|less|more|bat|nl|cut|wc|jq|python3?|/usr/bin/grep)( |$)'; then
                    # Only an operand that is a record is a read.
                    files=$(operands "$p")
                    while IFS= read -r f; do
                        [ -n "$f" ] || continue
                        eval_cache "$f" "$cdir" && continue
                        in_store "$f" && { shell="$p"; break; }
                    done <<OPS
$files
OPS
                fi
                ;;
        esac
    done <<EOF
$parts
EOF
    if [ -n "$history" ]; then
        if [ "$brief" -eq 1 ]; then
            log history brief "$history"
            exit 0
        fi
        [ "$mode" = pre ] || exit 0
        log history blocked "$history"
        printf '%s\n' "This document was migrated into trailmark and removed; its git history is not a source. Ask trailmark: \`trailmark find <words>\`, \`trailmark show <id>\`. If your brief names this read, put TRAIL_ALLOW=brief in the command." >&2
        exit 2
    fi
    if [ -n "$edit" ]; then
        log edit logged "$edit"
        advise "this edit"
    fi
    if [ -n "$shell" ]; then
        log shell "$([ "$brief" -eq 1 ] && echo brief || echo logged)" "$shell"
        [ "$brief" -eq 1 ] && exit 0
        advise "this command"
    fi
    exit 0
    ;;
esac
exit 0
