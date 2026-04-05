    .section .data
    .align 3
    .globl __boot_release_flag
__boot_release_flag:
    .dword 0

    .section .text.init
    .globl _start
_start:
    csrr a0, mhartid

    la t0, __boot_stack_top
    li t1, 16384
    mul t2, a0, t1
    sub sp, t0, t2

    beqz a0, 1f

2:
    la t0, __boot_release_flag
3:
    ld t1, 0(t0)
    beqz t1, 3b
    call start_secondary
    j 4f

1:
    call start_primary

4:
    j 4b
