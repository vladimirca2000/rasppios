/* Raspberry Pi 3B+ (AArch64).
 * O firmware carrega kernel8.img em 0x80000 e salta para o primeiro byte.
 * A pilha fica acima da imagem e cresce para baixo.
 */
ENTRY(_start)

SECTIONS
{
    . = 0x80000;

    .text : ALIGN(4096) {
        __text_start = .;
        KEEP(*(.text.boot))
        . = ALIGN(2048);
        KEEP(*(.text.vectors))
        *(.text .text.*)
        . = ALIGN(4096);
        __text_end = .;
    }

    .rodata : ALIGN(4096) {
        __rodata_start = .;
        *(.rodata .rodata.*)
        . = ALIGN(4096);
        __rodata_end = .;
    }

    .data : ALIGN(4096) {
        *(.data .data.*)
    }

    .bss (NOLOAD) : ALIGN(16) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(8);
        __bss_end = .;
    }

    /* 32 KiB por núcleo, 16 bytes de alinhamento. Cresce para baixo. */
    . = ALIGN(16);
    __stacks_start = .;
    . += 0x8000 * 4;
    __stacks_end = .;

    /DISCARD/ : {
        *(.comment)
        *(.gnu*)
        *(.eh_frame*)
        *(.note*)
    }
}
