#!/usr/bin/env bash
# Tracked AXP Product Edition lifecycle helper.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
cd "$repo_root"

usage() {
  local code="${1:-2}"
  echo "usage:" >&2
  echo "  scripts/swarm.sh new <number|AXP-E####>" >&2
  echo "  scripts/swarm.sh validate <AXP-E####>" >&2
  echo "  scripts/swarm.sh validate-assignments <path.tsv>" >&2
  echo "  scripts/swarm.sh status <AXP-E####>" >&2
  echo "  scripts/swarm.sh packet <AXP-E####> <E####-##>" >&2
  echo "  scripts/swarm.sh scoped <package> [--list]" >&2
  echo "  scripts/swarm.sh verify <AXP-E####> --quiescent" >&2
  exit "$code"
}

normalize_edition() {
  local input="$1"
  if [[ "$input" =~ ^AXP-E[0-9]{4}$ ]]; then
    printf '%s\n' "$input"
  elif [[ "$input" =~ ^[0-9]{1,4}$ ]]; then
    printf 'AXP-E%04d\n' "$((10#$input))"
  else
    echo "invalid edition identifier: $input" >&2
    return 1
  fi
}

edition_dir() {
  local edition
  edition="$(normalize_edition "$1")"
  printf 'editions/%s\n' "$edition"
}

validate_assignments() {
  local file="$1"
  local line_number=0
  local task wave status model effort owner paths dependencies
  local -a tasks=() waves=() statuses=() path_sets=() dependency_sets=()
  declare -A seen_tasks=() task_waves=() task_statuses=() wave_counts=()

  while IFS=$'\t' read -r task wave status model effort owner paths dependencies; do
    line_number=$((line_number + 1))
    if [[ $line_number -eq 1 ]]; then
      if [[ "$task" != "task" \
        || "$wave" != "wave" \
        || "$status" != "status" \
        || "$model" != "model" \
        || "$effort" != "effort" \
        || "$owner" != "owner" \
        || "$paths" != "writable_paths" \
        || "$dependencies" != "dependencies" ]]; then
        echo "$file: invalid header" >&2
        return 1
      fi
      continue
    fi
    [[ -z "$task" ]] && continue

    if [[ ! "$task" =~ ^E[0-9]{4}-[0-9]{2}$ ]]; then
      echo "$file:$line_number: invalid task id: $task" >&2
      return 1
    fi
    if [[ -n "${seen_tasks[$task]:-}" ]]; then
      echo "$file:$line_number: duplicate task id: $task" >&2
      return 1
    fi
    if [[ ! "$wave" =~ ^W[0-9]+$ ]]; then
      echo "$file:$line_number: invalid wave: $wave" >&2
      return 1
    fi
    case "$status" in
      pending|ready|active|blocked|review|done) ;;
      *) echo "$file:$line_number: invalid status: $status" >&2; return 1 ;;
    esac
    seen_tasks["$task"]=1
    task_waves["$task"]="$wave"
    task_statuses["$task"]="$status"
    if [[ "$status" != "pending" && "$owner" == "unassigned" ]]; then
      echo "$file:$line_number: $status task must have an assigned owner: $task" >&2
      return 1
    fi
    case "$model" in
      gpt-5.6-luna|gpt-5.6-terra|gpt-5.6-sol) ;;
      *) echo "$file:$line_number: invalid model: $model" >&2; return 1 ;;
    esac
    case "$effort" in
      low|medium|high|xhigh|max) ;;
      *) echo "$file:$line_number: invalid reasoning effort: $effort" >&2; return 1 ;;
    esac
    if [[ -z "$owner" || -z "$paths" || -z "$dependencies" ]]; then
      echo "$file:$line_number: owner, writable_paths, and dependencies are required" >&2
      return 1
    fi

    wave_counts["$wave"]=$(( ${wave_counts[$wave]:-0} + 1 ))
    if (( wave_counts[$wave] > 3 )); then
      echo "$file:$line_number: wave $wave exceeds the three-worker limit" >&2
      return 1
    fi

    local -a declared_paths=()
    local declared_path
    IFS=';' read -r -a declared_paths <<< "$paths"
    for declared_path in "${declared_paths[@]}"; do
      if [[ -z "$declared_path" \
        || "$declared_path" == /* \
        || "$declared_path" == "~"* \
        || "$declared_path" == *".."* \
        || "$declared_path" == "." \
        || "$declared_path" == "*" \
        || "$declared_path" == "**" \
        || "$declared_path" == "/" ]]; then
        echo "$file:$line_number: unsafe writable path: $declared_path" >&2
        return 1
      fi
    done

    tasks+=("$task")
    waves+=("$wave")
    statuses+=("$status")
    path_sets+=("$paths")
    dependency_sets+=("$dependencies")
  done < "$file"

  local i dependency task_wave dependency_wave
  local -a declared_dependencies=()
  for ((i = 0; i < ${#tasks[@]}; i++)); do
    [[ "${dependency_sets[$i]}" == "none" ]] && continue
    IFS=';' read -r -a declared_dependencies <<< "${dependency_sets[$i]}"
    for dependency in "${declared_dependencies[@]}"; do
      if [[ -z "${seen_tasks[$dependency]:-}" ]]; then
        echo "$file: ${tasks[$i]} has unknown dependency: $dependency" >&2
        return 1
      fi
      if [[ "$dependency" == "${tasks[$i]}" ]]; then
        echo "$file: ${tasks[$i]} cannot depend on itself" >&2
        return 1
      fi
      task_wave="${waves[$i]#W}"
      dependency_wave="${task_waves[$dependency]#W}"
      if (( 10#$dependency_wave >= 10#$task_wave )); then
        echo "$file: ${tasks[$i]} must depend on an earlier wave: $dependency" >&2
        return 1
      fi
      case "${statuses[$i]}" in
        ready|active|review|done)
          if [[ "${task_statuses[$dependency]}" != "done" ]]; then
            echo "$file: ${tasks[$i]} is ${statuses[$i]} but dependency $dependency is not done" >&2
            return 1
          fi
          ;;
      esac
    done
  done

  local j left_path right_path left_root right_root
  local -a left=() right=()
  for ((i = 0; i < ${#tasks[@]}; i++)); do
    for ((j = i + 1; j < ${#tasks[@]}; j++)); do
      [[ "${waves[$i]}" != "${waves[$j]}" ]] && continue
      IFS=';' read -r -a left <<< "${path_sets[$i]}"
      IFS=';' read -r -a right <<< "${path_sets[$j]}"
      for left_path in "${left[@]}"; do
        left_root="${left_path%/\*\*}"
        left_root="${left_root%/}"
        for right_path in "${right[@]}"; do
          right_root="${right_path%/\*\*}"
          right_root="${right_root%/}"
          if [[ "$left_root" == "$right_root" \
            || "$left_root" == "$right_root"/* \
            || "$right_root" == "$left_root"/* ]]; then
            echo "ownership overlap in ${waves[$i]}: ${tasks[$i]} ($left_path) and ${tasks[$j]} ($right_path)" >&2
            return 1
          fi
        done
      done
    done
  done
}

validate_edition() {
  local edition="$1"
  local dir
  dir="$(edition_dir "$edition")"
  local required=(
    charter.md
    workgraph.md
    ownership.md
    status.md
    decisions.md
    evidence.md
    retro.md
    assignments.tsv
  )
  local file
  for file in "${required[@]}"; do
    if [[ ! -f "$dir/$file" ]]; then
      echo "$dir: missing required artifact: $file" >&2
      return 1
    fi
  done
  if rtk git check-ignore -q "$dir"; then
    echo "$dir is ignored; edition control records must be tracked" >&2
    return 1
  fi
  if rtk rg -n 'AXP-E####|TBD: required before assignment' "$dir"; then
    echo "$dir contains unresolved assignment placeholders" >&2
    return 1
  fi
  validate_assignments "$dir/assignments.tsv"
  local expected_prefix="${edition#AXP-}-"
  local task wave status model effort owner paths dependencies
  while IFS=$'\t' read -r task wave status model effort owner paths dependencies; do
    [[ "$task" == "task" ]] && continue
    [[ -z "$task" ]] && continue
    if [[ "$task" != "$expected_prefix"* ]]; then
      echo "$dir/assignments.tsv: task $task does not belong to $edition" >&2
      return 1
    fi
    if [[ ! -f "$dir/tasks/$task.md" ]]; then
      echo "$dir: missing task packet: tasks/$task.md" >&2
      return 1
    fi
    if [[ ! -f "$dir/handoffs/$task.md" ]]; then
      echo "$dir: missing handoff record: handoffs/$task.md" >&2
      return 1
    fi
  done < "$dir/assignments.tsv"
  echo "$dir: valid"
}

new_edition() {
  local edition="$1"
  local dir
  edition="$(normalize_edition "$edition")"
  dir="editions/$edition"
  if [[ -e "$dir" ]]; then
    echo "$dir already exists; refusing to overwrite" >&2
    return 1
  fi
  rtk mkdir -p "$dir"
  rtk cp -R editions/_template/. "$dir/"
  local file task_prefix
  task_prefix="${edition#AXP-}"
  for file in "$dir"/*; do
    [[ -f "$file" ]] || continue
    rtk sed -i "s/AXP-E####/$edition/g" "$file"
    rtk sed -i "s/E####/$task_prefix/g" "$file"
  done
  rtk mkdir -p "$dir/tasks" "$dir/handoffs"
  rtk mv "$dir/task.md" "$dir/tasks/${task_prefix}-01.md"
  rtk mv "$dir/handoff.md" "$dir/handoffs/${task_prefix}-01.md"
  echo "created $dir"
  echo "complete $dir/charter.md and $dir/assignments.tsv before Gate A"
}

show_status() {
  local dir
  dir="$(edition_dir "$1")"
  if [[ ! -f "$dir/status.md" ]]; then
    echo "edition not found: $dir" >&2
    return 1
  fi
  rtk sed -n '1,220p' "$dir/status.md"
  echo
  echo "Assignments"
  while IFS=$'\t' read -r task wave status model effort owner paths dependencies; do
    [[ "$task" == "task" ]] && continue
    [[ -z "$task" ]] && continue
    printf '%-10s %-4s %-8s %-14s %-7s %s\n' \
      "$task" "$wave" "$status" "$model" "$effort" "$owner"
  done < "$dir/assignments.tsv"
}

show_packet() {
  local dir task found=false
  dir="$(edition_dir "$1")"
  task="$2"
  validate_edition "$1" >/dev/null
  if [[ ! -f "$dir/tasks/$task.md" ]]; then
    echo "task packet not found: $dir/tasks/$task.md" >&2
    return 1
  fi
  echo "Assignment"
  while IFS=$'\t' read -r row_task wave status model effort owner paths dependencies; do
    [[ "$row_task" == "$task" ]] || continue
    printf 'task=%s wave=%s status=%s model=%s effort=%s owner=%s dependencies=%s\n' \
      "$row_task" "$wave" "$status" "$model" "$effort" "$owner" "$dependencies"
    printf 'writable_paths=%s\n\n' "$paths"
    found=true
  done < "$dir/assignments.tsv"
  if [[ "$found" != true ]]; then
    echo "task is not assigned in $dir/assignments.tsv: $task" >&2
    return 1
  fi
  rtk sed -n '1,320p' "$dir/tasks/$task.md"
}

command="${1:-}"
case "$command" in
  help|-h|--help)
    usage 0
    ;;
  new)
    [[ $# -eq 2 ]] || usage
    new_edition "$2"
    ;;
  validate)
    [[ $# -eq 2 ]] || usage
    validate_edition "$2"
    ;;
  validate-assignments)
    [[ $# -eq 2 ]] || usage
    validate_assignments "$2"
    echo "$2: valid"
    ;;
  status)
    [[ $# -eq 2 ]] || usage
    show_status "$2"
    ;;
  packet)
    [[ $# -eq 3 ]] || usage
    show_packet "$2" "$3"
    ;;
  scoped)
    [[ $# -eq 2 || $# -eq 3 ]] || usage
    rtk scripts/test-scoped.sh "$2" "${3:-}"
    ;;
  verify)
    [[ $# -eq 3 && "$3" == "--quiescent" ]] || usage
    validate_edition "$2"
    rtk git diff --check
    rtk cargo fmt --check --all
    rtk cargo test --workspace
    ;;
  *) usage ;;
esac
