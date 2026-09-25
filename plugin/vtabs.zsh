# Source from .zshrc on each machine whose jobs should appear in Cmd+Z.
[[ -o interactive && -o zle ]] || return 0
(( ${+_vtabs_jobs_loaded} )) && return 0
typeset -g _vtabs_jobs_loaded=1
typeset -gi _vtabs_jobs_generation=0
typeset -g _vtabs_jobs_last=''
zmodload zsh/parameter
autoload -Uz add-zle-hook-widget add-zsh-hook

_vtabs_jobs_publish() {
  emulate -L zsh
  local ready=${1:-0} number state process command_text data encoded
  local count=0
  data=$'1\t'$$$'\t'$_vtabs_jobs_generation$'\t'$ready$'\t'${HOST//[[:cntrl:]]/ }
  for number in ${(onk)jobstates}; do
    state=${jobstates[$number]}
    process=${state#*:*:}
    process=${process%%=*}
    state=${state%%:*}
    [[ $state == (running|suspended) ]] || continue
    command_text=${jobtexts[$number]//[[:cntrl:]]/ }
    command_text=${command_text[1,256]}
    data+=$'\n'$number$'\t'$process$'\t'$state$'\t'$command_text
    (( ++count >= 128 )) && break
  done
  [[ $data == $_vtabs_jobs_last ]] && return 0
  _vtabs_jobs_last=$data
  encoded=$(print -rn -- "$data" | command base64)
  printf '\e]1337;SetUserVar=vtabs_jobs=%s\a' "${encoded//$'\n'/}"
}

_vtabs_jobs_start() {
  (( ++_vtabs_jobs_generation ))
  if [[ $CONTEXT == start && -o monitor ]]; then
    _vtabs_jobs_publish 1
  else
    _vtabs_jobs_publish 0
  fi
}

_vtabs_jobs_busy() { _vtabs_jobs_publish 0 }

_vtabs_jobs_request() {
  emulate -L zsh
  local request='' character state process
  local -a fields
  # The prefix is a ZLE binding; the remainder contains only numeric identities.
  while (( ${#request} < 96 )); do
    read -r -k 1 -t 1 character || return 0
    [[ $character == '~' ]] && break
    [[ $character == [0-9\;] ]] || return 0
    request+=$character
  done
  [[ $character == '~' ]] || return 0
  fields=(${(s.;.)request})
  [[ ${#fields} == 5 && $fields[1] == $$ && $fields[2] == $_vtabs_jobs_generation && $CONTEXT == start && -o monitor ]] || return 0
  if [[ $fields[5] == 0 ]]; then
    _vtabs_jobs_publish 1
    return 0
  fi
  [[ $fields[5] == [123] ]] || return 0
  [[ $fields[3] == <1-> && $fields[4] == <1-> ]] || return 0
  state=${jobstates[$fields[3]]}
  process=${state#*:*:}
  process=${process%%=*}
  state=${state%%:*}
  if [[ $state != (running|suspended) || $process != $fields[4] ]]; then
    zle -M 'Job no longer exists'
    _vtabs_jobs_publish 1
    return 0
  fi
  _vtabs_jobs_busy
  zle -I
  case $fields[5] in
    1) builtin fg %$fields[3] ;;
    2) builtin bg %$fields[3] ;;
    3) builtin kill -TERM %$fields[3] ;;
  esac
  _vtabs_jobs_publish 1
  zle reset-prompt
}

zle -N _vtabs_jobs_request
zle -N _vtabs_jobs_start
zle -N _vtabs_jobs_busy
add-zle-hook-widget line-init _vtabs_jobs_start
add-zle-hook-widget line-finish _vtabs_jobs_busy
add-zsh-hook preexec _vtabs_jobs_busy
for _vtabs_jobs_keymap in emacs viins vicmd; do
  bindkey -M $_vtabs_jobs_keymap $'\e[777;' _vtabs_jobs_request
done
unset _vtabs_jobs_keymap
