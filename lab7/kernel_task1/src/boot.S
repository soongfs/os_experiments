    .section .text.init
    .globl _start
_start:
    la sp, __kernel_stack_top
    call start_machine

1:
    j 1b

    .equ TRAP_FRAME_SIZE, 256
    .equ MSTATUS_MPP_MASK, 0x1800
    .equ MSTATUS_MPP_S, 0x0800
    .equ SSTATUS_SPP, 0x100
    .equ SSTATUS_SPIE, 0x20
    .equ SSTATUS_SIE, 0x2

    .equ SHM_PARENT_VA, 0x00402000
    .equ SHM_CHILD_VA, 0x00405000
    .equ SYS_FORK, 1
    .equ SYS_YIELD, 2
    .equ SYS_REPORT, 3
    .equ SYS_EXIT, 4
    .equ SYS_SHM_CREATE, 5
    .equ SYS_SHM_MAP, 6
    .equ INITIAL_VALUE, 0x1111222233334444
    .equ CHILD_WRITE_VALUE, 0x0c0ffee000000001

    .section .text
    .globl enter_supervisor
enter_supervisor:
    csrw mepc, a0

    csrr t0, mstatus
    li t1, MSTATUS_MPP_MASK
    not t1, t1
    and t0, t0, t1
    li t1, MSTATUS_MPP_S
    or t0, t0, t1
    csrw mstatus, t0

    mv sp, a1
    mret

    .globl enter_user_task
enter_user_task:
    mv t6, a0
    csrw sscratch, a1

    ld t0, 31 * 8(t6)
    csrw sepc, t0

    csrr t0, sstatus
    li t1, SSTATUS_SPP | SSTATUS_SPIE | SSTATUS_SIE
    not t1, t1
    and t0, t0, t1
    li t1, SSTATUS_SPIE
    or t0, t0, t1
    csrw sstatus, t0

    ld ra, 0 * 8(t6)
    ld gp, 1 * 8(t6)
    ld tp, 2 * 8(t6)
    ld t0, 3 * 8(t6)
    ld t1, 4 * 8(t6)
    ld t2, 5 * 8(t6)
    ld s0, 6 * 8(t6)
    ld s1, 7 * 8(t6)
    ld a0, 8 * 8(t6)
    ld a1, 9 * 8(t6)
    ld a2, 10 * 8(t6)
    ld a3, 11 * 8(t6)
    ld a4, 12 * 8(t6)
    ld a5, 13 * 8(t6)
    ld a6, 14 * 8(t6)
    ld a7, 15 * 8(t6)
    ld s2, 16 * 8(t6)
    ld s3, 17 * 8(t6)
    ld s4, 18 * 8(t6)
    ld s5, 19 * 8(t6)
    ld s6, 20 * 8(t6)
    ld s7, 21 * 8(t6)
    ld s8, 22 * 8(t6)
    ld s9, 23 * 8(t6)
    ld s10, 24 * 8(t6)
    ld s11, 25 * 8(t6)
    ld t3, 26 * 8(t6)
    ld t4, 27 * 8(t6)
    ld t5, 28 * 8(t6)
    ld sp, 30 * 8(t6)
    ld t6, 29 * 8(t6)
    sret

    .align 4
    .globl machine_trap_entry
machine_trap_entry:
    csrrw sp, mscratch, sp
    addi sp, sp, -TRAP_FRAME_SIZE

    sd ra, 0 * 8(sp)
    sd gp, 1 * 8(sp)
    sd tp, 2 * 8(sp)
    sd t0, 3 * 8(sp)
    sd t1, 4 * 8(sp)
    sd t2, 5 * 8(sp)
    sd s0, 6 * 8(sp)
    sd s1, 7 * 8(sp)
    sd a0, 8 * 8(sp)
    sd a1, 9 * 8(sp)
    sd a2, 10 * 8(sp)
    sd a3, 11 * 8(sp)
    sd a4, 12 * 8(sp)
    sd a5, 13 * 8(sp)
    sd a6, 14 * 8(sp)
    sd a7, 15 * 8(sp)
    sd s2, 16 * 8(sp)
    sd s3, 17 * 8(sp)
    sd s4, 18 * 8(sp)
    sd s5, 19 * 8(sp)
    sd s6, 20 * 8(sp)
    sd s7, 21 * 8(sp)
    sd s8, 22 * 8(sp)
    sd s9, 23 * 8(sp)
    sd s10, 24 * 8(sp)
    sd s11, 25 * 8(sp)
    sd t3, 26 * 8(sp)
    sd t4, 27 * 8(sp)
    sd t5, 28 * 8(sp)
    sd t6, 29 * 8(sp)

    csrr t0, mscratch
    sd t0, 30 * 8(sp)
    csrr t0, mepc
    sd t0, 31 * 8(sp)

    mv a0, sp
    call handle_machine_trap

    ld t0, 31 * 8(sp)
    csrw mepc, t0
    ld t0, 30 * 8(sp)
    csrw mscratch, t0

    ld ra, 0 * 8(sp)
    ld gp, 1 * 8(sp)
    ld tp, 2 * 8(sp)
    ld t0, 3 * 8(sp)
    ld t1, 4 * 8(sp)
    ld t2, 5 * 8(sp)
    ld s0, 6 * 8(sp)
    ld s1, 7 * 8(sp)
    ld a0, 8 * 8(sp)
    ld a1, 9 * 8(sp)
    ld a2, 10 * 8(sp)
    ld a3, 11 * 8(sp)
    ld a4, 12 * 8(sp)
    ld a5, 13 * 8(sp)
    ld a6, 14 * 8(sp)
    ld a7, 15 * 8(sp)
    ld s2, 16 * 8(sp)
    ld s3, 17 * 8(sp)
    ld s4, 18 * 8(sp)
    ld s5, 19 * 8(sp)
    ld s6, 20 * 8(sp)
    ld s7, 21 * 8(sp)
    ld s8, 22 * 8(sp)
    ld s9, 23 * 8(sp)
    ld s10, 24 * 8(sp)
    ld s11, 25 * 8(sp)
    ld t3, 26 * 8(sp)
    ld t4, 27 * 8(sp)
    ld t5, 28 * 8(sp)
    ld t6, 29 * 8(sp)

    addi sp, sp, TRAP_FRAME_SIZE
    csrrw sp, mscratch, sp
    mret

    .align 4
    .globl supervisor_trap_entry
supervisor_trap_entry:
    csrrw sp, sscratch, sp
    addi sp, sp, -TRAP_FRAME_SIZE

    sd ra, 0 * 8(sp)
    sd gp, 1 * 8(sp)
    sd tp, 2 * 8(sp)
    sd t0, 3 * 8(sp)
    sd t1, 4 * 8(sp)
    sd t2, 5 * 8(sp)
    sd s0, 6 * 8(sp)
    sd s1, 7 * 8(sp)
    sd a0, 8 * 8(sp)
    sd a1, 9 * 8(sp)
    sd a2, 10 * 8(sp)
    sd a3, 11 * 8(sp)
    sd a4, 12 * 8(sp)
    sd a5, 13 * 8(sp)
    sd a6, 14 * 8(sp)
    sd a7, 15 * 8(sp)
    sd s2, 16 * 8(sp)
    sd s3, 17 * 8(sp)
    sd s4, 18 * 8(sp)
    sd s5, 19 * 8(sp)
    sd s6, 20 * 8(sp)
    sd s7, 21 * 8(sp)
    sd s8, 22 * 8(sp)
    sd s9, 23 * 8(sp)
    sd s10, 24 * 8(sp)
    sd s11, 25 * 8(sp)
    sd t3, 26 * 8(sp)
    sd t4, 27 * 8(sp)
    sd t5, 28 * 8(sp)
    sd t6, 29 * 8(sp)

    csrr t0, sscratch
    sd t0, 30 * 8(sp)
    csrr t0, sepc
    sd t0, 31 * 8(sp)

    mv a0, sp
    call handle_supervisor_trap

    ld t0, 31 * 8(sp)
    csrw sepc, t0
    ld t0, 30 * 8(sp)
    csrw sscratch, t0

    ld ra, 0 * 8(sp)
    ld gp, 1 * 8(sp)
    ld tp, 2 * 8(sp)
    ld t0, 3 * 8(sp)
    ld t1, 4 * 8(sp)
    ld t2, 5 * 8(sp)
    ld s0, 6 * 8(sp)
    ld s1, 7 * 8(sp)
    ld a0, 8 * 8(sp)
    ld a1, 9 * 8(sp)
    ld a2, 10 * 8(sp)
    ld a3, 11 * 8(sp)
    ld a4, 12 * 8(sp)
    ld a5, 13 * 8(sp)
    ld a6, 14 * 8(sp)
    ld a7, 15 * 8(sp)
    ld s2, 16 * 8(sp)
    ld s3, 17 * 8(sp)
    ld s4, 18 * 8(sp)
    ld s5, 19 * 8(sp)
    ld s6, 20 * 8(sp)
    ld s7, 21 * 8(sp)
    ld s8, 22 * 8(sp)
    ld s9, 23 * 8(sp)
    ld s10, 24 * 8(sp)
    ld s11, 25 * 8(sp)
    ld t3, 26 * 8(sp)
    ld t4, 27 * 8(sp)
    ld t5, 28 * 8(sp)
    ld t6, 29 * 8(sp)

    addi sp, sp, TRAP_FRAME_SIZE
    csrrw sp, sscratch, sp
    sret

    .align 4
    .globl __user_program_start
__user_program_start:
    li s0, SHM_PARENT_VA
    li s1, SHM_CHILD_VA

    li a7, SYS_SHM_CREATE
    ecall
    mv s2, a0
    bltz a0, fail

    mv a0, s2
    mv a1, s0
    li a7, SYS_SHM_MAP
    ecall
    bltz a0, fail

    li t0, INITIAL_VALUE
    sd t0, 0(s0)
    mv s3, t0

    li a7, SYS_FORK
    ecall
    bnez a0, parent_path

child_path:
    mv a0, s2
    mv a1, s1
    li a7, SYS_SHM_MAP
    ecall
    bltz a0, fail

    ld s4, 0(s1)
    li t1, CHILD_WRITE_VALUE
    sd t1, 0(s1)
    ld s5, 0(s1)

    mv a1, s4
    mv a2, s5
    li a3, 0
    li a7, SYS_REPORT
    ecall

    li a0, 0
    li a7, SYS_EXIT
    ecall

parent_path:
    li a7, SYS_YIELD
    ecall

    ld s6, 0(s0)

    mv a1, s3
    mv a2, s6
    li a3, CHILD_WRITE_VALUE
    li a7, SYS_REPORT
    ecall

    li a0, 0
    li a7, SYS_EXIT
    ecall

fail:
    li a0, 1
    li a7, SYS_EXIT
    ecall

    .globl __user_program_end
__user_program_end:
