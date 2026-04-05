OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY
{
    RAM (rwx) : ORIGIN = 0x80000000, LENGTH = 16M
}

SECTIONS
{
    . = ORIGIN(RAM);

    .text : ALIGN(4) {
        *(.text.init)
        *(.text .text.*)
        *(.rodata .rodata.*)
    } > RAM

    .data : ALIGN(16) {
        *(.sdata .sdata.*)
        *(.data .data.*)
        *(.got .got.*)
    } > RAM

    .bss (NOLOAD) : ALIGN(16) {
        __bss_start = .;
        *(.sbss .sbss.*)
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    } > RAM

    . = ALIGN(16);
    __boot_stack_bottom = .;
    . += 64K;
    __boot_stack_top = .;

    __interrupt_stack_bottom = .;
    . += 32K;
    __interrupt_stack_top = .;

    __image_end = .;

    /DISCARD/ : {
        *(.eh_frame .eh_frame.*)
        *(.comment .comment.*)
    }
}
