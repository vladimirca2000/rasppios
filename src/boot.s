// Entrada AArch64. O firmware (ou o QEMU -kernel) chega aqui em 0x80000.
// Cada núcleo calcula a própria pilha a partir de MPIDR_EL1.Aff0.
// Só o núcleo 0 zera o BSS e entra no kernel. Os outros, quando o
// núcleo 0 gravar o spin table, entram em rust_secondary.

.section .text.boot, "ax"
.global _start
.type _start, %function

.section .data
.balign 4
.global CORE_ARRIVED
CORE_ARRIVED:
    .byte 0, 0, 0, 0

.section .text.boot, "ax"

_start:
    mrs     x0, mpidr_el1
    and     x0, x0, #0xff
    cmp     x0, #4
    b.hs    park

    // SP_ELx, não SP_EL0: a exceção síncrona deste nível usa o offset 0x200.
    msr     spsel, #1

    // Núcleo N: [__stacks_end - (N + 1) * 32 KiB, __stacks_end - N * 32 KiB).
    mov     x1, #0x8000
    mul     x2, x0, x1
    ldr     x1, =__stacks_end
    sub     sp, x1, x2

    msr     daifset, #0xf

    ldr     x1, =CORE_ARRIVED
    mov     w2, #1
    strb    w2, [x1, x0]
    dsb     sy

    cbnz    x0, secondary

    ldr     x0, =__bss_start
    ldr     x1, =__bss_end
    cmp     x0, x1
    b.hs    2f
1:
    str     xzr, [x0], #8
    cmp     x0, x1
    b.lo    1b
2:
    bl      rust_main

secondary:
    bl      rust_secondary

park:
    wfe
    b       park
