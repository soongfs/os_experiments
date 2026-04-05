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

    mret

    .globl enter_user_mode
enter_user_mode:
    csrw sscratch, a2
    csrw sepc, a0

    csrr t0, sstatus
    li t1, SSTATUS_SPP
    not t1, t1
    and t0, t0, t1
    li t1, SSTATUS_SPIE
    or t0, t0, t1
    csrw sstatus, t0

    mv sp, a1
    sret

    .align 4
    .globl trap_entry
trap_entry:
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
    call handle_trap

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
