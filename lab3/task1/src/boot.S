    .section .text.init
    .globl _start
_start:
    la sp, __kernel_stack_top
    call start_kernel

1:
    j 1b

    .equ MSTATUS_MPP_MASK, 0x1800
    .equ MSTATUS_MPIE, 0x80
    .equ MSTATUS_FS_MASK, 0x6000
    .equ MSTATUS_FS_DIRTY, 0x6000

    .equ TF_RA, 0
    .equ TF_GP, 8
    .equ TF_TP, 16
    .equ TF_T0, 24
    .equ TF_T1, 32
    .equ TF_T2, 40
    .equ TF_S0, 48
    .equ TF_S1, 56
    .equ TF_A0, 64
    .equ TF_A1, 72
    .equ TF_A2, 80
    .equ TF_A3, 88
    .equ TF_A4, 96
    .equ TF_A5, 104
    .equ TF_A6, 112
    .equ TF_A7, 120
    .equ TF_S2, 128
    .equ TF_S3, 136
    .equ TF_S4, 144
    .equ TF_S5, 152
    .equ TF_S6, 160
    .equ TF_S7, 168
    .equ TF_S8, 176
    .equ TF_S9, 184
    .equ TF_S10, 192
    .equ TF_S11, 200
    .equ TF_T3, 208
    .equ TF_T4, 216
    .equ TF_T5, 224
    .equ TF_T6, 232
    .equ TF_USER_SP, 240
    .equ TF_MEPC, 248
    .equ TF_F0, 256
    .equ TF_F1, 264
    .equ TF_F2, 272
    .equ TF_F3, 280
    .equ TF_F4, 288
    .equ TF_F5, 296
    .equ TF_F6, 304
    .equ TF_F7, 312
    .equ TF_F8, 320
    .equ TF_F9, 328
    .equ TF_F10, 336
    .equ TF_F11, 344
    .equ TF_F12, 352
    .equ TF_F13, 360
    .equ TF_F14, 368
    .equ TF_F15, 376
    .equ TF_F16, 384
    .equ TF_F17, 392
    .equ TF_F18, 400
    .equ TF_F19, 408
    .equ TF_F20, 416
    .equ TF_F21, 424
    .equ TF_F22, 432
    .equ TF_F23, 440
    .equ TF_F24, 448
    .equ TF_F25, 456
    .equ TF_F26, 464
    .equ TF_F27, 472
    .equ TF_F28, 480
    .equ TF_F29, 488
    .equ TF_F30, 496
    .equ TF_F31, 504
    .equ TF_FCSR, 512
    .equ TF_RESERVED, 520
    .equ TRAP_FRAME_SIZE, 528

    .section .text
    .globl enter_task
enter_task:
    mv t6, a0
    csrw mscratch, a1

    csrr t0, mstatus
    li t1, MSTATUS_MPP_MASK | MSTATUS_FS_MASK
    not t1, t1
    and t0, t0, t1
    li t1, MSTATUS_MPIE | MSTATUS_FS_DIRTY
    or t0, t0, t1
    csrw mstatus, t0

    ld t0, TF_MEPC(t6)
    csrw mepc, t0
    ld t0, TF_FCSR(t6)
    csrw fcsr, t0

    fld f0, TF_F0(t6)
    fld f1, TF_F1(t6)
    fld f2, TF_F2(t6)
    fld f3, TF_F3(t6)
    fld f4, TF_F4(t6)
    fld f5, TF_F5(t6)
    fld f6, TF_F6(t6)
    fld f7, TF_F7(t6)
    fld f8, TF_F8(t6)
    fld f9, TF_F9(t6)
    fld f10, TF_F10(t6)
    fld f11, TF_F11(t6)
    fld f12, TF_F12(t6)
    fld f13, TF_F13(t6)
    fld f14, TF_F14(t6)
    fld f15, TF_F15(t6)
    fld f16, TF_F16(t6)
    fld f17, TF_F17(t6)
    fld f18, TF_F18(t6)
    fld f19, TF_F19(t6)
    fld f20, TF_F20(t6)
    fld f21, TF_F21(t6)
    fld f22, TF_F22(t6)
    fld f23, TF_F23(t6)
    fld f24, TF_F24(t6)
    fld f25, TF_F25(t6)
    fld f26, TF_F26(t6)
    fld f27, TF_F27(t6)
    fld f28, TF_F28(t6)
    fld f29, TF_F29(t6)
    fld f30, TF_F30(t6)
    fld f31, TF_F31(t6)

    ld ra, TF_RA(t6)
    ld gp, TF_GP(t6)
    ld tp, TF_TP(t6)
    ld s0, TF_S0(t6)
    ld s1, TF_S1(t6)
    ld s2, TF_S2(t6)
    ld s3, TF_S3(t6)
    ld s4, TF_S4(t6)
    ld s5, TF_S5(t6)
    ld s6, TF_S6(t6)
    ld s7, TF_S7(t6)
    ld s8, TF_S8(t6)
    ld s9, TF_S9(t6)
    ld s10, TF_S10(t6)
    ld s11, TF_S11(t6)
    ld a0, TF_A0(t6)
    ld a1, TF_A1(t6)
    ld a2, TF_A2(t6)
    ld a3, TF_A3(t6)
    ld a4, TF_A4(t6)
    ld a5, TF_A5(t6)
    ld a6, TF_A6(t6)
    ld a7, TF_A7(t6)
    ld t0, TF_T0(t6)
    ld t1, TF_T1(t6)
    ld t2, TF_T2(t6)
    ld t3, TF_T3(t6)
    ld t4, TF_T4(t6)
    ld t5, TF_T5(t6)
    ld sp, TF_USER_SP(t6)
    ld t6, TF_T6(t6)
    mret

    .align 4
    .globl trap_entry
trap_entry:
    csrrw sp, mscratch, sp
    addi sp, sp, -TRAP_FRAME_SIZE

    sd ra, TF_RA(sp)
    sd gp, TF_GP(sp)
    sd tp, TF_TP(sp)
    sd t0, TF_T0(sp)
    sd t1, TF_T1(sp)
    sd t2, TF_T2(sp)
    sd s0, TF_S0(sp)
    sd s1, TF_S1(sp)
    sd a0, TF_A0(sp)
    sd a1, TF_A1(sp)
    sd a2, TF_A2(sp)
    sd a3, TF_A3(sp)
    sd a4, TF_A4(sp)
    sd a5, TF_A5(sp)
    sd a6, TF_A6(sp)
    sd a7, TF_A7(sp)
    sd s2, TF_S2(sp)
    sd s3, TF_S3(sp)
    sd s4, TF_S4(sp)
    sd s5, TF_S5(sp)
    sd s6, TF_S6(sp)
    sd s7, TF_S7(sp)
    sd s8, TF_S8(sp)
    sd s9, TF_S9(sp)
    sd s10, TF_S10(sp)
    sd s11, TF_S11(sp)
    sd t3, TF_T3(sp)
    sd t4, TF_T4(sp)
    sd t5, TF_T5(sp)
    sd t6, TF_T6(sp)

    csrr t0, mscratch
    sd t0, TF_USER_SP(sp)
    csrr t0, mepc
    sd t0, TF_MEPC(sp)

    fsd f0, TF_F0(sp)
    fsd f1, TF_F1(sp)
    fsd f2, TF_F2(sp)
    fsd f3, TF_F3(sp)
    fsd f4, TF_F4(sp)
    fsd f5, TF_F5(sp)
    fsd f6, TF_F6(sp)
    fsd f7, TF_F7(sp)
    fsd f8, TF_F8(sp)
    fsd f9, TF_F9(sp)
    fsd f10, TF_F10(sp)
    fsd f11, TF_F11(sp)
    fsd f12, TF_F12(sp)
    fsd f13, TF_F13(sp)
    fsd f14, TF_F14(sp)
    fsd f15, TF_F15(sp)
    fsd f16, TF_F16(sp)
    fsd f17, TF_F17(sp)
    fsd f18, TF_F18(sp)
    fsd f19, TF_F19(sp)
    fsd f20, TF_F20(sp)
    fsd f21, TF_F21(sp)
    fsd f22, TF_F22(sp)
    fsd f23, TF_F23(sp)
    fsd f24, TF_F24(sp)
    fsd f25, TF_F25(sp)
    fsd f26, TF_F26(sp)
    fsd f27, TF_F27(sp)
    fsd f28, TF_F28(sp)
    fsd f29, TF_F29(sp)
    fsd f30, TF_F30(sp)
    fsd f31, TF_F31(sp)

    csrr t0, fcsr
    sd t0, TF_FCSR(sp)
    sd zero, TF_RESERVED(sp)

    mv a0, sp
    call rust_handle_trap

    ld t0, TF_MEPC(sp)
    csrw mepc, t0
    ld t0, TF_FCSR(sp)
    csrw fcsr, t0
    ld t0, TF_USER_SP(sp)
    csrw mscratch, t0

    fld f0, TF_F0(sp)
    fld f1, TF_F1(sp)
    fld f2, TF_F2(sp)
    fld f3, TF_F3(sp)
    fld f4, TF_F4(sp)
    fld f5, TF_F5(sp)
    fld f6, TF_F6(sp)
    fld f7, TF_F7(sp)
    fld f8, TF_F8(sp)
    fld f9, TF_F9(sp)
    fld f10, TF_F10(sp)
    fld f11, TF_F11(sp)
    fld f12, TF_F12(sp)
    fld f13, TF_F13(sp)
    fld f14, TF_F14(sp)
    fld f15, TF_F15(sp)
    fld f16, TF_F16(sp)
    fld f17, TF_F17(sp)
    fld f18, TF_F18(sp)
    fld f19, TF_F19(sp)
    fld f20, TF_F20(sp)
    fld f21, TF_F21(sp)
    fld f22, TF_F22(sp)
    fld f23, TF_F23(sp)
    fld f24, TF_F24(sp)
    fld f25, TF_F25(sp)
    fld f26, TF_F26(sp)
    fld f27, TF_F27(sp)
    fld f28, TF_F28(sp)
    fld f29, TF_F29(sp)
    fld f30, TF_F30(sp)
    fld f31, TF_F31(sp)

    ld ra, TF_RA(sp)
    ld gp, TF_GP(sp)
    ld tp, TF_TP(sp)
    ld t0, TF_T0(sp)
    ld t1, TF_T1(sp)
    ld t2, TF_T2(sp)
    ld s0, TF_S0(sp)
    ld s1, TF_S1(sp)
    ld a0, TF_A0(sp)
    ld a1, TF_A1(sp)
    ld a2, TF_A2(sp)
    ld a3, TF_A3(sp)
    ld a4, TF_A4(sp)
    ld a5, TF_A5(sp)
    ld a6, TF_A6(sp)
    ld a7, TF_A7(sp)
    ld s2, TF_S2(sp)
    ld s3, TF_S3(sp)
    ld s4, TF_S4(sp)
    ld s5, TF_S5(sp)
    ld s6, TF_S6(sp)
    ld s7, TF_S7(sp)
    ld s8, TF_S8(sp)
    ld s9, TF_S9(sp)
    ld s10, TF_S10(sp)
    ld s11, TF_S11(sp)
    ld t3, TF_T3(sp)
    ld t4, TF_T4(sp)
    ld t5, TF_T5(sp)
    ld t6, TF_T6(sp)

    addi sp, sp, TRAP_FRAME_SIZE
    csrrw sp, mscratch, sp
    mret

    .align 4
    .globl fp_stress_loop
fp_stress_loop:
    addi sp, sp, -64
    fsd f18, 0(sp)
    fsd f19, 8(sp)
    fsd f20, 16(sp)
    fsd f21, 24(sp)
    fsd f22, 32(sp)
    fsd f23, 40(sp)
    fsd f24, 48(sp)
    fsd f25, 56(sp)

    fld f0, 0(a0)
    fld f1, 8(a0)
    fld f2, 16(a0)
    fld f3, 24(a0)
    fld f4, 32(a0)
    fld f5, 40(a0)
    fld f6, 48(a0)
    fld f7, 56(a0)

    fld f10, 0(a1)
    fld f11, 8(a1)
    fld f12, 16(a1)
    fld f13, 24(a1)
    fld f14, 32(a1)
    fld f15, 40(a1)
    fld f16, 48(a1)
    fld f17, 56(a1)

    fld f18, 0(a2)
    fld f19, 8(a2)
    fld f20, 16(a2)
    fld f21, 24(a2)
    fld f22, 32(a2)
    fld f23, 40(a2)
    fld f24, 48(a2)
    fld f25, 56(a2)

    mv t0, a3
1:
    fmadd.d f0, f0, f10, f18
    fmadd.d f1, f1, f11, f19
    fmadd.d f2, f2, f12, f20
    fmadd.d f3, f3, f13, f21
    fmadd.d f4, f4, f14, f22
    fmadd.d f5, f5, f15, f23
    fmadd.d f6, f6, f16, f24
    fmadd.d f7, f7, f17, f25
    addi t0, t0, -1
    bnez t0, 1b

    fmv.x.d t0, f0
    fmv.x.d t1, f1
    xor t0, t0, t1
    fmv.x.d t1, f2
    add t0, t0, t1
    fmv.x.d t1, f3
    xor t0, t0, t1
    fmv.x.d t1, f4
    add t0, t0, t1
    fmv.x.d t1, f5
    xor t0, t0, t1
    fmv.x.d t1, f6
    add t0, t0, t1
    fmv.x.d t1, f7
    xor a0, t0, t1

    fld f18, 0(sp)
    fld f19, 8(sp)
    fld f20, 16(sp)
    fld f21, 24(sp)
    fld f22, 32(sp)
    fld f23, 40(sp)
    fld f24, 48(sp)
    fld f25, 56(sp)
    addi sp, sp, 64
    ret
