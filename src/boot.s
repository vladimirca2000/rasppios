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
.global MMU_READY
MMU_READY:
    .byte 0

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

    // O firmware entrega EL2 e `make run` entrega EL3. EL0 só existe
    // debaixo de EL1, então cada núcleo desce antes de tocar no kernel.
    // HCR_EL2.RW (bit 31) pede EL1 em AArch64. IMO fica em 0: a IRQ
    // física chega em EL1. SCTLR_EL1 sai com a MMU desligada e os RES1
    // (bits 29, 28, 23, 22, 20 e 11) ligados. SPSR 0x3c5 é EL1h com
    // DAIF mascarado.
    mrs     x9, CurrentEL
    lsr     x9, x9, #2
    cmp     x9, #1
    b.eq    el1_ready

    mov     x10, sp
    msr     sp_el1, x10
    ldr     x10, =0x30d00800
    msr     sctlr_el1, x10
    ldr     x12, =exception_vectors
    movz    x10, #0x8000, lsl #16
    mov     x11, #0x3c5
    cmp     x9, #3
    b.eq    from_el3

    msr     vbar_el2, x12
    msr     hcr_el2, x10
    msr     spsr_el2, x11
    // CPTR_EL2 com TFP limpo e os RES1 ligados. Sem isto o SIMD do EL1
    // sobe para EL2. O valor 0x33ff é o reset sem o bit 10.
    mov     x13, #0x33ff
    msr     cptr_el2, x13
    adr     x10, el1_ready
    msr     elr_el2, x10
    isb
    eret

from_el3:
    // SCR_EL3: NS, RW e os RES1 dos bits 5:4. IRQ, FIQ e EA não sobem a EL3.
    msr     vbar_el3, x12
    mrs     x12, scr_el3
    mov     x13, #0x431
    orr     x12, x12, x13
    mov     x13, #0xe
    bic     x12, x12, x13
    msr     scr_el3, x12
    msr     hcr_el2, x10
    msr     spsr_el3, x11
    mov     x13, #0x33ff
    msr     cptr_el2, x13
    adr     x10, el1_ready
    msr     elr_el3, x10
    isb
    eret

el1_ready:
    msr     spsel, #1
    // CPACR_EL1.FPEN = 01: o EL1 usa o SIMD que o compilador emite
    // para zerar buffers. O EL0 continua sem acesso e cai no vetor.
    movz    x13, #0x10, lsl #16
    msr     cpacr_el1, x13
    isb
    ldr     x10, =exception_vectors
    msr     vbar_el1, x10
    isb

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
    // No ELF o QEMU solta os quatro núcleos juntos. A MMU só pode ligar
    // depois que o núcleo 0 gravou as tabelas e publicou este byte.
    ldr     x1, =MMU_READY
1:
    ldrb    w2, [x1]
    cbnz    w2, 2f
    wfe
    b       1b
2:
    bl      rust_secondary

park:
    wfe
    b       park
