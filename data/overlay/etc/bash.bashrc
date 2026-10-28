[ -z "$PS1" ] && return
[ -n "$HAMIX_BASHRC_DONE" ] && return
HAMIX_BASHRC_DONE=1

shopt -s checkwinsize histappend
HISTSIZE=1000
HISTFILESIZE=2000
HISTCONTROL=ignoredups

if [ "$EUID" = 0 ]; then
    __hamix_user='\[\033[91m\]\u@\h\[\033[0m\]'
    __hamix_mark='#'
else
    __hamix_user='\[\033[92m\]\u@\h\[\033[0m\]'
    __hamix_mark='\$'
fi

__hamix_prompt() {
    local code=$?
    local status=
    [ "$code" -ne 0 ] && status="\[\033[91m\][$code]\[\033[0m\] "
    PS1="$status$__hamix_user:\[\033[94m\]\w\[\033[0m\]$__hamix_mark "
}
PROMPT_COMMAND=__hamix_prompt

PS2='\[\033[90m\]>\[\033[0m\] '

alias ls='ls --color=auto'
alias ll='ls -l'
alias la='ls -la'
alias l='ls -1'
alias ..='cd ..'

command_not_found_handle() {
    local entry="/var/lib/hamix/commands/$1"
    if [ -f "$entry" ]; then
        local fields
        IFS=$'\t' read -r -a fields < "$entry"
        shift
        "${fields[2]}" "${fields[@]:3}" "$@"
        return $?
    fi
    printf 'bash: %s: command not found\n' "$1" >&2
    return 127
}
