#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
CGROUP_ROOT=/sys/fs/cgroup
MEMORY_MAX=${MEMORY_MAX:-192M}
SWAP_MAX=${SWAP_MAX:-768M}
RUN_ID=${RUN_ID:-default}
BIN="$SCRIPT_DIR/lazy_swap_trigger"

if [[ ! -x "$BIN" ]]; then
    echo "missing binary: $BIN" >&2
    echo "run 'make' in $SCRIPT_DIR first" >&2
    exit 1
fi

if [[ ! -w "$CGROUP_ROOT" ]]; then
    echo "cgroup root is not writable: $CGROUP_ROOT" >&2
    exit 1
fi

if ! grep -qw memory "$CGROUP_ROOT/cgroup.subtree_control"; then
    echo "+memory" > "$CGROUP_ROOT/cgroup.subtree_control"
fi

read_vmstat_value() {
    local key=$1
    awk -v wanted="$key" '$1 == wanted { print $2 }' /proc/vmstat
}

print_cgroup_summary() {
    local cg=$1
    local prefix=$2
    local memory_current memory_swap_current
    memory_current=$(<"$cg/memory.current")
    memory_swap_current=$(<"$cg/memory.swap.current")

    echo "$prefix memory.current=$memory_current memory.swap.current=$memory_swap_current"
    awk '
        $1 == "anon" ||
        $1 == "swapcached" ||
        $1 == "pgfault" ||
        $1 == "pgmajfault" ||
        $1 == "pgscan" ||
        $1 == "pgsteal" {
            printf("%s %s=%s\n", prefix, $1, $2);
        }
    ' prefix="$prefix" "$cg/memory.stat"
}

run_case() {
    local label=$1
    local working_set_mib=$2
    local step_mib=$3
    local revisit_passes=$4
    local cg="$CGROUP_ROOT/lab4_task2_${RUN_ID}_${label}_$$"
    local pgfault_before pgmajfault_before pswpin_before pswpout_before
    local pgfault_after pgmajfault_after pswpin_after pswpout_after

    mkdir "$cg"
    echo "$MEMORY_MAX" > "$cg/memory.max"
    echo "$SWAP_MAX" > "$cg/memory.swap.max"

    pgfault_before=$(read_vmstat_value pgfault)
    pgmajfault_before=$(read_vmstat_value pgmajfault)
    pswpin_before=$(read_vmstat_value pswpin)
    pswpout_before=$(read_vmstat_value pswpout)

    echo "=== case=$label working_set=${working_set_mib}MiB step=${step_mib}MiB revisit_passes=$revisit_passes memory.max=$MEMORY_MAX memory.swap.max=$SWAP_MAX ==="
    print_cgroup_summary "$cg" "[cgroup-before/$label]"
    echo "[vmstat-before/$label] pgfault=$pgfault_before pgmajfault=$pgmajfault_before pswpin=$pswpin_before pswpout=$pswpout_before"

    bash -lc "echo \$\$ > '$cg/cgroup.procs'; exec '$BIN' --label '$label' --working-set-mib '$working_set_mib' --step-mib '$step_mib' --revisit-passes '$revisit_passes'"

    pgfault_after=$(read_vmstat_value pgfault)
    pgmajfault_after=$(read_vmstat_value pgmajfault)
    pswpin_after=$(read_vmstat_value pswpin)
    pswpout_after=$(read_vmstat_value pswpout)

    print_cgroup_summary "$cg" "[cgroup-after/$label]"
    echo "[vmstat-after/$label] pgfault=$pgfault_after pgmajfault=$pgmajfault_after pswpin=$pswpin_after pswpout=$pswpout_after"
    echo "[vmstat-delta/$label] pgfault=$((pgfault_after - pgfault_before)) pgmajfault=$((pgmajfault_after - pgmajfault_before)) pswpin=$((pswpin_after - pswpin_before)) pswpout=$((pswpout_after - pswpout_before))"

    rmdir "$cg"
    echo
}

run_case low 128 16 1
run_case medium 384 16 2
run_case high 640 16 3
